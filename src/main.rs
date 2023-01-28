use std::f32::consts::PI;
use std::{borrow::Cow, num::NonZeroU64};

use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::renderer::RenderQueue;
use bevy::{
    prelude::*,
    render::{
        render_asset::{PrepareAssetLabel, RenderAssets},
        render_graph::{self, RenderGraph},
        render_resource::{
            BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout,
            BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingResource, BindingType, Buffer,
            BufferBinding, BufferBindingType, BufferInitDescriptor, BufferUsages,
            CachedComputePipelineId, CachedPipelineState, ComputePassDescriptor,
            ComputePipelineDescriptor, Extent3d, PipelineCache, ShaderStages, StorageTextureAccess,
            TextureDimension, TextureFormat, TextureUsages, TextureViewDimension, WgpuFeatures,
        },
        renderer::RenderDevice,
        settings::WgpuSettings,
        RenderApp, RenderStage,
    },
};
use rand::Rng;

use bevy_inspector_egui::prelude::*;
use bevy_inspector_egui::quick::ResourceInspectorPlugin;

const SIZE_FACTOR: (u32, u32) = (240, 135);
const BASIC_WIDTH: f32 = 1280.;
const WORKGROUP_SIZE: (u32, u32) = (8, 8);
const SIZE: (u32, u32) = (
    SIZE_FACTOR.0 * WORKGROUP_SIZE.0,
    SIZE_FACTOR.1 * WORKGROUP_SIZE.1,
);
const SCREEN_SIZE: (f32, f32) = (
    BASIC_WIDTH,
    BASIC_WIDTH * SIZE_FACTOR.1 as f32 / SIZE_FACTOR.0 as f32,
);
const AGENT_NUM: usize = 500_000;
const SHADER_CONSTANTS: ShaderConstants = ShaderConstants {
    width: SIZE.0,
    height: SIZE.1,
    agent_num: AGENT_NUM as u32,
};
const AGENT_VER_PRESET1: AgentVerb = AgentVerb {
    move_speed: 0.33,
    fade_speed: 0.003,
    diffuse_speed: 0.05,
    sensor_size: 1,
    sensor_distance: 35.,
    turning_speed: 0.2,
};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
struct ShaderConstants {
    width: u32,
    height: u32,
    agent_num: u32,
}

#[repr(C)]
#[derive(
    Clone,
    Copy,
    bytemuck::Zeroable,
    bytemuck::Pod,
    Default,
    Reflect,
    Resource,
    ExtractResource,
    InspectorOptions,
)]
#[reflect(Resource, InspectorOptions)]
struct AgentVerb {
    #[inspector(min = 0.)]
    move_speed: f32,
    #[inspector(min = 0.)]
    fade_speed: f32,
    #[inspector(min = 0.)]
    diffuse_speed: f32,
    #[inspector(min = 1)]
    sensor_size: u32,
    #[inspector(min=1., max=SCREEN_SIZE.0.min(SCREEN_SIZE.1))]
    sensor_distance: f32,
    #[inspector(min = 0.)]
    turning_speed: f32,
}
#[derive(Resource)]
struct Pipelines {
    bind_group_layout: BindGroupLayout,
    update_pipeline: CachedComputePipelineId,
    trail_map_update_pipeline: CachedComputePipelineId,
}
#[derive(Resource)]
struct Binding(BindGroup);
#[derive(Clone, Resource)]
struct Buffers {
    constants_buffer: Buffer,
    agent_buffer: Buffer,
    agent_setting_buffer: Buffer,
}
#[repr(C, align(16))]
#[derive(Copy, bytemuck::Zeroable, bytemuck::Pod)]
struct Agent {
    position: [f32; 2],
    angle: f32,
    _padding: [u8; 4],
}
impl Clone for Agent {
    fn clone(&self) -> Self {
        Self {
            position: self.position.clone(),
            angle: self.angle.clone(),
            _padding: [0; 4],
        }
    }
}

fn main() {
    let mut app = App::new();
    app
    .insert_resource(WgpuSettings {
        features: WgpuFeatures::BUFFER_BINDING_ARRAY,
        ..default()
    })
    .insert_resource(ClearColor(Color::BLACK))
    .add_plugins(DefaultPlugins.set(WindowPlugin {
        window: WindowDescriptor {
            width: SCREEN_SIZE.0,
            height: SCREEN_SIZE.1,
            // present_mode: bevy::window::PresentMode::Immediate,
            ..default()
        },
        ..default()
    }))
    .add_plugin(ResourceInspectorPlugin::<AgentVerb>::default())
    .add_plugin(ExtractResourcePlugin::<RenderTarget>::default())
    .add_plugin(ExtractResourcePlugin::<AgentVerb>::default())
    .add_startup_system(setup)
    .add_system(handle_input);

    let agent_setting = AGENT_VER_PRESET1;
    app.insert_resource(agent_setting);

    let render_app = app.sub_app_mut(RenderApp);
    let render_device = render_app.world.resource::<RenderDevice>();
    let bind_group_layout = render_device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            BindGroupLayoutEntry {
                binding: 0,
                ty: BindingType::StorageTexture {
                    access: StorageTextureAccess::ReadWrite,
                    format: TextureFormat::Rgba8Unorm,
                    view_dimension: TextureViewDimension::D2,
                },
                visibility: ShaderStages::COMPUTE,
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: Some(
                        NonZeroU64::new(std::mem::size_of::<ShaderConstants>() as u64).unwrap(),
                    ),
                },
                visibility: ShaderStages::COMPUTE,
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 2,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: Some(
                        NonZeroU64::new((std::mem::size_of::<Agent>() * AGENT_NUM) as u64).unwrap(),
                    ),
                },
                visibility: ShaderStages::COMPUTE,
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 3,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: Some(
                        NonZeroU64::new((std::mem::size_of::<AgentVerb>()) as u64).unwrap(),
                    ),
                },
                visibility: ShaderStages::COMPUTE,
                count: None,
            },
        ],
    });
    let constants_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: None,
        contents: bytemuck::bytes_of(&SHADER_CONSTANTS),
        usage: BufferUsages::UNIFORM | BufferUsages::STORAGE,
    });

    // create agents
    let mut agents = Vec::with_capacity(AGENT_NUM);
    let mut rng = rand::thread_rng();
    for _ in 0..AGENT_NUM {
        // spaws randomly
        // agents.push(Agent {
        //     position: [rng.gen_range(0..SIZE.0) as f32, rng.gen_range(0..SIZE.1) as f32],
        //     angel: rng.gen_range(0f32..PI * 2.),
        //     _padding: [0; 4],
        // });

        let center = Vec2::new(SIZE.0 as f32 / 2., SIZE.1 as f32 / 2.);

        // spaws in a circle
        let r = SIZE.1 as f32 / 2. * rng.gen::<f32>().sqrt();
        let theta = rng.gen::<f32>() * PI * 2.;
        let position = Vec2::new(center.x + r * theta.cos(), center.y + r * theta.sin());
        agents.push(Agent {
            position: [position.x, position.y],
            angle: (center - position)
                .normalize()
                .y
                .atan2((center - position).normalize().x),
            _padding: [0; 4],
        });

        // spaws at the center
        // agents.push(Agent {
        //     position: [center.x, center.y],
        //     angle: rng.gen::<f32>() * PI * 2.,
        //     _padding: [0; 4]
        // });
    }
    let agent_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(agents.as_slice()),
        usage: BufferUsages::UNIFORM | BufferUsages::STORAGE,
    });

    let agent_setting_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: None,
        contents: bytemuck::bytes_of(&agent_setting),
        usage: BufferUsages::UNIFORM | BufferUsages::STORAGE | BufferUsages::COPY_DST,
    });

    render_app.insert_resource(Buffers {
        constants_buffer,
        agent_buffer,
        agent_setting_buffer,
    });

    let shader = render_app
        .world
        .resource::<AssetServer>()
        .load("shader.wgsl");

    let mut pipeline_cache = render_app.world.resource_mut::<PipelineCache>();
    let update_pipeline = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: None,
        layout: Some(vec![bind_group_layout.clone()]),
        shader: shader.clone(),
        shader_defs: vec![],
        entry_point: Cow::from("update"),
    });
    let trail_map_update_pipeline =
        pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: None,
            layout: Some(vec![bind_group_layout.clone()]),
            shader,
            shader_defs: vec![],
            entry_point: Cow::from("update_trail_map"),
        });

    render_app.world.insert_resource(Pipelines {
        bind_group_layout,
        update_pipeline,
        trail_map_update_pipeline,
    });

    render_app.add_system_to_stage(
        RenderStage::Prepare,
        prepare_binding_group.after(PrepareAssetLabel::AssetPrepare),
    );

    let mut graph = render_app.world.resource_mut::<RenderGraph>();
    graph.add_node("shader_node", RenderNode::default());
    graph
        .add_node_edge("shader_node", bevy::render::main_graph::node::CAMERA_DRIVER)
        .unwrap();

    app.run();
}

fn prepare_binding_group(
    mut commands: Commands,
    gpu_images: Res<RenderAssets<Image>>,
    image: Res<RenderTarget>,
    buffers: Res<Buffers>,
    bind_group_layout: Res<Pipelines>,
    render_device: Res<RenderDevice>,
) {
    let bind_group = render_device.create_bind_group(&BindGroupDescriptor {
        label: None,
        layout: &bind_group_layout.bind_group_layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(&gpu_images[&image].texture_view),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::Buffer(BufferBinding {
                    buffer: &buffers.constants_buffer,
                    offset: 0,
                    size: Some(
                        NonZeroU64::new(std::mem::size_of::<ShaderConstants>() as u64).unwrap(),
                    ),
                }),
            },
            BindGroupEntry {
                binding: 2,
                resource: BindingResource::Buffer(BufferBinding {
                    buffer: &buffers.agent_buffer,
                    offset: 0,
                    size: Some(
                        NonZeroU64::new((std::mem::size_of::<Agent>() * AGENT_NUM) as u64).unwrap(),
                    ),
                }),
            },
            BindGroupEntry {
                binding: 3,
                resource: BindingResource::Buffer(BufferBinding {
                    buffer: &buffers.agent_setting_buffer,
                    offset: 0,
                    size: Some(NonZeroU64::new((std::mem::size_of::<AgentVerb>()) as u64).unwrap()),
                }),
            },
        ],
    });
    commands.insert_resource(Binding(bind_group));
}

fn handle_input(
    mut camera_query: Query<(
        &mut OrthographicProjection,
        &mut Transform,
        With<MainCamera>,
    )>,
    mouse_button: Res<Input<MouseButton>>,
    mut mouse_motion: EventReader<MouseMotion>,
    mut mouse_wheel: EventReader<MouseWheel>,
) {
    let mut camera = camera_query.get_single_mut().unwrap();
    if mouse_button.pressed(MouseButton::Left) {
        for mov in mouse_motion.iter() {
            camera.1.translation += Vec3::new(-mov.delta.x, mov.delta.y, 0.) * 1. * camera.0.scale;
        }
    }

    for wheel in mouse_wheel.iter() {
        camera.0.scale = (camera.0.scale * (1. - wheel.y * 0.2)).clamp(0.01, 3.);
    }
}

#[derive(Deref, Clone, Resource, ExtractResource)]
struct RenderTarget(Handle<Image>);
#[derive(Component)]
struct SpriteTarget;
#[derive(Component)]
struct MainCamera;

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut image = Image::new_fill(
        Extent3d {
            width: SIZE.0,
            height: SIZE.1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8Unorm,
    );
    image.texture_descriptor.usage =
        TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING | TextureUsages::STORAGE_BINDING;
    let image = images.add(image);

    for i in -1..=1i32 {
        for j in -1..=1i32 {
            commands
                .spawn(SpriteBundle {
                    sprite: Sprite {
                        custom_size: Some(Vec2::new(SIZE.0 as f32, SIZE.1 as f32)),
                        ..default()
                    },
                    texture: image.clone(),
                    transform: Transform::from_translation(Vec3::new(
                        (SIZE.0 as i32 * i) as f32,
                        (SIZE.1 as i32 * j) as f32,
                        1.,
                    )),
                    ..default()
                })
                .insert(SpriteTarget);
        }
    }

    let camera = Camera2dBundle {
        projection: OrthographicProjection {
            scale: (SIZE.0 as f32 / SCREEN_SIZE.0).min(SIZE.1 as f32 / SCREEN_SIZE.1),
            ..default()
        },
        ..default()
    };
    commands.spawn(camera).insert(MainCamera);

    commands.insert_resource(RenderTarget(image));
}

struct RenderNode {
    state: NodeState,
}
enum NodeState {
    Loading,
    Update,
}

impl Default for RenderNode {
    fn default() -> Self {
        RenderNode {
            state: NodeState::Loading,
        }
    }
}

impl render_graph::Node for RenderNode {
    fn update(&mut self, world: &mut World) {
        let pipelines = world.resource::<Pipelines>();
        let pipeline_cache = world.resource::<PipelineCache>();

        match self.state {
            NodeState::Loading => {
                if let CachedPipelineState::Ok(_) =
                    pipeline_cache.get_compute_pipeline_state(pipelines.update_pipeline)
                {
                    self.state = NodeState::Update;
                }
            }
            NodeState::Update => {}
        }
    }

    fn run(
        &self,
        _graph: &mut render_graph::RenderGraphContext,
        render_context: &mut bevy::render::renderer::RenderContext,
        world: &World,
    ) -> Result<(), render_graph::NodeRunError> {
        let pipeline_cache = world.resource::<PipelineCache>();
        let pipelines = world.resource::<Pipelines>();
        let binding_group = world.resource::<Binding>();

        let mut pass = render_context
            .command_encoder
            .begin_compute_pass(&ComputePassDescriptor::default());

        pass.set_bind_group(0, &binding_group.0, &[]);

        match self.state {
            NodeState::Loading => {}
            NodeState::Update => {
                let pipeline = pipeline_cache
                    .get_compute_pipeline(pipelines.update_pipeline)
                    .unwrap();
                pass.set_pipeline(pipeline);
                pass.dispatch_workgroups(AGENT_NUM as u32 / 16 + 1, 1, 1);

                drop(pass);

                let mut pass = render_context
                    .command_encoder
                    .begin_compute_pass(&ComputePassDescriptor::default());
                pass.set_bind_group(0, &binding_group.0, &[]);
                let pipeline = pipeline_cache
                    .get_compute_pipeline(pipelines.trail_map_update_pipeline)
                    .unwrap();
                pass.set_pipeline(pipeline);
                pass.dispatch_workgroups(SIZE.0 / WORKGROUP_SIZE.0, SIZE.1 / WORKGROUP_SIZE.1, 1);
            }
        }

        let queue = world.resource::<RenderQueue>();
        let buffers = world.resource::<Buffers>();
        let agent_verb = world.resource::<AgentVerb>();

        queue.write_buffer(
            &buffers.agent_setting_buffer,
            0,
            bytemuck::bytes_of(agent_verb),
        );

        Ok(())
    }
}

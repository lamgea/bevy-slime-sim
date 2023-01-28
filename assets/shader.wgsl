@group(0) @binding(0)
var texture: texture_storage_2d<rgba8unorm, read_write>;

struct Constants {
    width: u32,
    height: u32,
    agent_num: u32,
};
@group(0) @binding(1)
var<uniform> constants: Constants;

struct AgentSetting {
    move_speed: f32,
    fade_speed: f32,
    diffuse_speed: f32,
    sensor_size: u32,
    sensor_distance: f32,
    turing_speed: f32,
};
@group(0) @binding(3)
var<uniform> agent_settings: AgentSetting;

let PI: f32 = 3.141592653589793;

fn hash(in: u32) -> u32 {
    var x = in;
    x = x ^ x >> 17u;
    x = x * 0xed5ad4bbu;
    x = x ^ x >> 11u;
    x = x * 0xac4c1b51u;
    x = x ^ x >> 15u;
    x = x * 0x31848babu;
    x = x ^ x >> 14u;
    return x;
}
fn hashf(x: u32) -> f32 {
    return f32(hash(x)) / 4294967295.0;
}

struct Agent {
    position: vec2<f32>,
    radian: f32,
};

struct Agents {
    agents: array<Agent>,
};
@group(0) @binding(2)
var<storage, read_write> agents: Agents;

fn lerp(v0: vec4<f32>, v1: vec4<f32>, t: f32) -> vec4<f32> {
  return v0 + t * (v1 - v0);
}
fn bound_checkf(coord: vec2<f32>) -> vec2<f32> {
    var result = coord;

    let width = f32(constants.width);
    let height = f32(constants.height);

    // bound 1
    if (result.x >= width) {
        result.x = result.x - width;
    }
    if (result.x < 0.) {
        result.x = result.x + width;
    }
    if (result.y >= height) {
        result.y = result.y - height;
    }
    if (result.y < 0.) {
        result.y = result.y + height;
    }

    return result;
}
fn bound_checki(coord: vec2<i32>) -> vec2<i32> {
    var result = coord;

    let width = i32(constants.width);
    let height = i32(constants.height);

    if (result.x >= width) {
        result.x = result.x - width;
    }
    if (result.x < 0) {
        result.x = result.x + width;
    }
    if (result.y >= height) {
        result.y = result.y - height;
    }
    if (result.y < 0) {
        result.y = result.y + height;
    }

    return result;
}
fn bound_checkb(coord: vec2<u32>) -> bool {
    return coord.x < constants.width && coord.x >= 0u && coord.y < constants.height && coord.y >= 0u;
}

fn normalize_angle(angels: vec2<f32>) -> vec2<f32> {
    let two_pi = PI * 2.;
    return angels - two_pi * floor((angels + PI) / two_pi);
}

fn sense(agent: Agent, sensor_radian_offset: f32) -> f32 {
    let sensor_radian = agent.radian + sensor_radian_offset;
    let sensor_dir =  vec2<f32>(cos(sensor_radian), sin(sensor_radian));
    let sensor_center = vec2<i32>(sensor_dir * agent_settings.sensor_distance + agent.position);

    var sum = 0.;
    let sensor_size = i32(agent_settings.sensor_size);
    for (var i = -sensor_size; i <= sensor_size;  i=i+1) {
        for (var j = -sensor_size; j <= sensor_size; j=j+1) {
            let sense_pos = sensor_center + vec2<i32>(i, j);

            sum = sum + textureLoad(texture, bound_checki(sense_pos)).a;
            // if (bound_checkb(vec2<u32>(sense_pos))) {
            //     sum = sum + textureLoad(texture, sense_pos).a;
            // }
        }
    }
    return sum;
}
fn sense_sector(agent: Agent, sensor_radian_range: vec2<f32>) -> f32 {
    let relative_range = normalize_angle(sensor_radian_range + agent.radian);
    let front = vec2<f32>(cos(agent.radian), sin(agent.radian));

    var sum = 0.;
    if (relative_range.x < relative_range.y) {
        for (var i = -agent_settings.sensor_distance; i <= agent_settings.sensor_distance;  i=i+1.) {
            for (var j = -agent_settings.sensor_distance; j <= agent_settings.sensor_distance; j=j+1.) {
                let sense_pos = agent.position + vec2<f32>(i, j);

                // whether is inside the circle
                if (distance(sense_pos, agent.position) > agent_settings.sensor_distance) { continue; }
                // whether is inside the range
                let pixel_dir = sense_pos - agent.position;
                let radian = atan2(pixel_dir.y, pixel_dir.x);
                if (radian > relative_range.x && radian < relative_range.y) {
                    sum = sum + textureLoad(texture, bound_checki(vec2<i32>(sense_pos))).a;

                    // if (bound_checkb(vec2<u32>(sense_pos))) {
                    //     sum = sum + textureLoad(texture, vec2<i32>(sense_pos)).a;
                    // }
                }
            }
        }
    } else {
        for (var i = -agent_settings.sensor_distance; i <= agent_settings.sensor_distance;  i=i+1.) {
            for (var j = -agent_settings.sensor_distance; j <= agent_settings.sensor_distance; j=j+1.) {
                let sense_pos = agent.position + vec2<f32>(i, j);

                // whether is inside the circle
                if (distance(sense_pos, agent.position) > agent_settings.sensor_distance) { continue; }
                // whether is inside the range
                let pixel_dir = sense_pos - agent.position;
                let radian = atan2(pixel_dir.y, pixel_dir.x);
                if (radian > relative_range.x || radian < relative_range.y) {
                    sum = sum + textureLoad(texture, bound_checki(vec2<i32>(sense_pos))).a;

                    // if (bound_checkb(vec2<u32>(sense_pos))) {
                    //     sum = sum + textureLoad(texture, vec2<i32>(sense_pos)).a;
                    // }
                }
            }
        }
    }
    
    return sum;
}

@compute @workgroup_size(16, 1, 1)
fn update(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= constants.agent_num) { return; }

    let width = f32(constants.width);
    let height = f32(constants.height);
    let agent = agents.agents[id.x];
    let ramdonf = hashf(id.x + u32(agent.position.x + agent.position.y * width));

    // turing
    let sensor_radian_space = PI / 4.;

    // let front_weight = sense_sector(agent, vec2<f32>(-sensor_radian_space / 2., sensor_radian_space / 2.));
    // let left_weight = sense_sector(agent, vec2<f32>(-sensor_radian_space * 3. / 2., -sensor_radian_space / 2.));
    // let right_weight = sense_sector(agent, vec2<f32>(sensor_radian_space / 2., sensor_radian_space * 3. / 2.));
    let front_weight = sense(agent, 0.);
    let left_weight = sense(agent, -sensor_radian_space);
    let right_weight = sense(agent, sensor_radian_space);

    if (front_weight > left_weight && front_weight > right_weight) {
        // radian doesn't change
    } else if (front_weight < left_weight && front_weight < right_weight) {
        // ramdonly change
        agents.agents[id.x].radian = agent.radian + (ramdonf - 0.5) * PI * agent_settings.turing_speed;
    } else if (right_weight > left_weight) {
        // turn right
        agents.agents[id.x].radian = agent.radian + agent_settings.turing_speed * ramdonf;
    } else if (left_weight > right_weight) {
        // turn left
        agents.agents[id.x].radian = agent.radian - agent_settings.turing_speed * ramdonf;
    }

    // moving
    let position = agent.position;
    let radian = agent.radian;
    var new_position = vec2<f32>(cos(radian), sin(radian)) * agent_settings.move_speed + position;

    let new_position = bound_checkf(new_position);

    // if (new_position.x >= width) {
    //     agents.agents[id.x].radian = ramdonf * PI + PI / 2.;
    // }
    // if (new_position.x < 0.) {
    //     agents.agents[id.x].radian = ramdonf * PI - PI / 2.;
    // }
    // if (new_position.y >= height) {
    //     agents.agents[id.x].radian = ramdonf * PI - PI;
    // }
    // if (new_position.y < 0.) {
    //     agents.agents[id.x].radian = ramdonf * PI;
    // }

    // new_position = clamp(vec2<f32>(0.), new_position, vec2<f32>(width, height) - 0.01);

    agents.agents[id.x].position = new_position;
    textureStore(texture, vec2<i32>(new_position), vec4<f32>(1.))
}

@compute @workgroup_size(8, 8, 1)
fn update_trail_map(@builtin(global_invocation_id) id: vec3<u32>) {
    let pos = vec2<i32>(id.xy);
    let value = textureLoad(texture, pos);

    var sum = vec4<f32>(0.);
    for (var i = -1; i <= 1; i=i+1) {
        for (var j = -1; j <= 1; j=j+1) {
            var sample = vec2<i32>(pos.x + i, pos.y + j);

            sum = sum + textureLoad(texture, bound_checki(sample));
            // if (bound_checkb(vec2<u32>(sample))) {
            //     sum = sum + textureLoad(texture, sample);
            // }
        }
    }
    
    let diffused_value = lerp(value, sum / 9., agent_settings.diffuse_speed);
    let final_value = max(vec4<f32>(0.), diffused_value - agent_settings.fade_speed);

    textureStore(texture, pos, final_value);
}

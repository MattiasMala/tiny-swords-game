// OWNER: C1 (terrain) — sea plane: pack colour #47aba9, lighter shallows near the coast, gentle pixel-snapped wave
// glints, StandardMaterial lighting (day/night, cliff shadows), fog of war, distance fog.

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    mesh_view_bindings::globals,
};

@if(PREPASS_PIPELINE)
import bevy_pbr::{
    prepass::io::{VertexOutput, FragmentOutput},
    deferred::functions::deferred_output,
};
@else
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

struct WaterParams {
    map_size: vec4<f32>,
    fog_color: vec4<f32>,
    shallow: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> params: WaterParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var fog_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var fog_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var shore_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var shore_sampler: sampler;

fn fog_of_war(c: vec3<f32>, s: f32) -> vec3<f32> {
    let l = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    let grey = mix(c, vec3<f32>(l), clamp(s * 1.6, 0.0, 0.7));
    return mix(grey, params.fog_color.rgb, s);
}

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

// Short horizontal glint dashes drifting slowly, on a 1/16-tile pixel grid (pixel-art water sparkle).
fn glints(xz: vec2<f32>, t: f32) -> f32 {
    let px = floor(xz * 16.0) / 16.0;
    // dashes: cells of 1.5 × 0.5 tiles, each with one dash that fades in and out
    let cell_size = vec2<f32>(1.5, 0.5);
    let shifted = px + vec2<f32>(t * 0.05, 0.0);
    let cell = floor(shifted / cell_size);
    let local = shifted / cell_size - cell;
    let r = hash2(cell);
    let phase = fract(t * 0.12 + r);
    let life = smoothstep(0.0, 0.25, phase) * (1.0 - smoothstep(0.45, 0.7, phase));
    let dash_x = abs(local.x - (0.2 + 0.6 * hash2(cell + 7.0)));
    let dash_y = abs(local.y - 0.5);
    let on = step(dash_x, 0.12 + 0.08 * r) * step(dash_y, 0.07);
    return on * life * step(0.45, r);
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let xz = in.world_position.xz;
    let uv = xz / params.map_size.xy;
    let fog_s = textureSample(fog_tex, fog_sampler, uv).r * params.map_size.z;
    let shore = textureSample(shore_tex, shore_sampler, uv).r;
    let t = globals.time;

    // shallows: lighter / greener within ~2 cells of land, quantised in 4 bands (pixel-art look)
    let shallow_k = clamp(1.0 - shore * 1.6, 0.0, 1.0);
    let banded = floor(shallow_k * 4.0 + 0.5) / 4.0;
    var base = pbr_input.material.base_color.rgb;
    base = mix(base, params.shallow.rgb, banded * 0.55);
    // glints, fewer near the coast (the foam already animates there)
    let g = glints(xz, t) * (0.35 + 0.65 * shore);
    base = base + vec3<f32>(0.10, 0.13, 0.12) * g;
    pbr_input.material.base_color = vec4<f32>(base, 1.0);
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color,
    );

    var out: FragmentOutput;
@if(PREPASS_PIPELINE) {
    out = deferred_output(in, pbr_input);
} @else {
    out.color = apply_pbr_lighting(pbr_input);
    out.color = vec4<f32>(fog_of_war(out.color.rgb, fog_s), out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
}
    return out;
}

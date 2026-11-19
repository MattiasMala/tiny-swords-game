// OWNER: C1 (terrain) — shore foam: `Water Foam.png` frames as a texture array, frame = floor(t * fps) mod 16 for
// every quad (synchronised surf, spec 09 §4.2), lit, fog of war, alpha-blended over the sea.

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

struct FoamParams {
    map_size: vec4<f32>,
    fog_color: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> params: FoamParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var frames: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var frames_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var fog_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var fog_sampler: sampler;

const FRAME_TEXELS: f32 = 192.0;

fn crisp_uv(uv: vec2<f32>) -> vec2<f32> {
    let p = uv * FRAME_TEXELS;
    let seam = floor(p + 0.5);
    let fw = clamp(fwidth(p), vec2<f32>(1e-4), vec2<f32>(1.0));
    let q = seam + clamp((p - seam) / fw, vec2<f32>(-0.5), vec2<f32>(0.5));
    return q / FRAME_TEXELS;
}

fn fog_of_war(c: vec3<f32>, s: f32) -> vec3<f32> {
    let l = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    let grey = mix(c, vec3<f32>(l), clamp(s * 1.6, 0.0, 0.7));
    return mix(grey, params.fog_color.rgb, s);
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let uv = in.uv;
    let ddx = dpdx(uv);
    let ddy = dpdy(uv);
    let suv = crisp_uv(uv);
    let fog_uv = in.world_position.xz / params.map_size.xy;
    let fog_s = textureSample(fog_tex, fog_sampler, fog_uv).r * params.map_size.z;
    let frame = i32(floor(globals.time * params.map_size.w)) % 16;
    let texel = textureSampleGrad(frames, frames_sampler, suv, frame, ddx, ddy);

    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color * texel,
    );

    var out: FragmentOutput;
@if(PREPASS_PIPELINE) {
    out = deferred_output(in, pbr_input);
} @else {
    out.color = apply_pbr_lighting(pbr_input);
    // distance fog (and in-shader tonemapping) first: unexplored ground stays very dark, without haze on it
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    out.color = vec4<f32>(fog_of_war(out.color.rgb, fog_s), out.color.a);
}
    return out;
}

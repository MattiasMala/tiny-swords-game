// OWNER: C1 (terrain) — voxel terrain: texture-array tiles (layer in uv_b.x), crisp pixel art without shimmering
// (texel-snapped UVs + mip-mapped trilinear sampling), grass lip on cliff walls, overlay tiles, StandardMaterial
// lighting/shadows, fog of war, distance fog.
//
// Vertex data (see `terrain_mesh::ChunkMesh`): uv = tile UV; uv_b.x = layer; uv_b.y = aux layer (top faces: ground
// under an overlay tile; walls: grass lip) or -1; color.r = brightness; color.g = walls: distance below the wall
// top in tiles (lip where < LIP_H), others >= 1.

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
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

struct TerrainParams {
    map_size: vec4<f32>,
    fog_color: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> params: TerrainParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var tiles: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var tiles_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var fog_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var fog_sampler: sampler;

const TILE_TEXELS: f32 = 64.0;
// grass lip: rows 40..63 of the elevated bottom-edge cell (24 px)
const LIP_H: f32 = 0.375;
const LIP_V0: f32 = 0.625;

// Texel-snapped UV: sharp texel edges when magnified (1 px anti-aliased seam), plain UV when minified.
fn crisp_uv(uv: vec2<f32>) -> vec2<f32> {
    let p = uv * TILE_TEXELS;
    let seam = floor(p + 0.5);
    let fw = clamp(fwidth(p), vec2<f32>(1e-4), vec2<f32>(1.0));
    let q = seam + clamp((p - seam) / fw, vec2<f32>(-0.5), vec2<f32>(0.5));
    return q / TILE_TEXELS;
}

// Fog of war: s = 0 visible, 0.42 explored (darker, desaturated), 1 never explored (near-black navy).
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
    let fog_uv = in.world_position.xz / params.map_size.xy;
    let fog_s = textureSample(fog_tex, fog_sampler, fog_uv).r * params.map_size.z;
    let layer = i32(in.uv_b.x + 0.5);
    let aux = in.uv_b.y;
    let lip_t = in.color.g;
    var texel: vec4<f32>;
    if (aux >= 0.0 && lip_t < LIP_H) {
        // grass lip at the top of a cliff wall (same texel density as the wall: v grows downwards)
        let luv = vec2<f32>(uv.x, LIP_V0 + lip_t);
        texel = textureSampleGrad(tiles, tiles_sampler, crisp_uv(luv), i32(aux + 0.5), ddx, ddy);
        // the lip is grass hanging over the edge: light it (almost) like the top face so the rim reads as one
        pbr_input.N = normalize(pbr_input.N * 0.3 + vec3<f32>(0.0, 1.0, 0.0));
    } else {
        texel = textureSampleGrad(tiles, tiles_sampler, crisp_uv(uv), layer, ddx, ddy);
        // overlay tiles (grass outline over a lower-priority ground) show the `aux` layer through alpha 0
        if (aux >= 0.0 && lip_t >= 1.0) {
            let under = textureSampleGrad(tiles, tiles_sampler, crisp_uv(uv), i32(aux + 0.5), ddx, ddy);
            texel = vec4<f32>(mix(under.rgb, texel.rgb, texel.a), 1.0);
        }
    }

    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        vec4<f32>(texel.rgb * in.color.r, 1.0),
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

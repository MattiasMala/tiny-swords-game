// OWNER: C2 (sprites / light) — "diorama" post pass of light level 2 (spec 07 §6, addenda L3/L4): tilt-shift bands
// (top 22 % / bottom 18 % of the screen fade into a blurred copy, mask alpha 1 - t²) and the 5 drifting sun shafts
// (§6.1). Runs after tonemapping (strictly after Bevy's bloom and vignette passes, see `light.rs`).

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;

struct DioramaParams {
    // top band fraction, bottom band fraction, blur radius (px at 720 rows), band strength
    band: vec4<f32>,
    // sun strength (0 = no shafts), lean, time (s), unused
    shafts: vec4<f32>,
    // linear shaft colour × gain, unused
    shaft_color: vec4<f32>,
}

@group(0) @binding(2) var<uniform> params: DioramaParams;

const GOLDEN: f32 = 2.39996323;
const TAPS: i32 = 14;

fn band_weight(uv_y: f32) -> f32 {
    let top = params.band.x;
    let bottom = params.band.y;
    var w = 0.0;
    if (top > 0.0001) {
        let t = uv_y / top;
        if (t < 1.0) {
            w = max(w, 1.0 - t * t);
        }
    }
    if (bottom > 0.0001) {
        let t = (1.0 - uv_y) / bottom;
        if (t < 1.0) {
            w = max(w, 1.0 - t * t);
        }
    }
    return w * params.band.w;
}

// Horizontal profile 0 → 1 → 0 across the shaft, vertical mask 1 (top) → 0.5 (70 %) → 0 (bottom).
fn shaft(px: vec2<f32>, dims: vec2<f32>, i: f32) -> f32 {
    let w = dims.x;
    let h = dims.y;
    let t = params.shafts.z;
    let lean = params.shafts.y;
    var x = (i * 0.19 + t * 0.006 + sin(i * 12.7) * 0.08) % 1.2;
    x = ((x + 1.2) % 1.2 - 0.1) * w;
    let sw = (0.1 + 0.07 * sin(i * 3.1)) * w;
    let a = 0.085 * params.shafts.x * (0.6 + 0.4 * sin(t * 0.35 + i * 2.1));
    // X = (x - lean*H*0.5) + lx + lean*Y  →  lx = X - x + lean*H*0.5 - lean*Y
    let lx = px.x - x + lean * h * 0.5 - lean * px.y;
    let u = (lx + sw * 0.5) / sw;
    if (u <= 0.0 || u >= 1.0) {
        return 0.0;
    }
    let v = (px.y + 0.05 * h) / (1.1 * h);
    var vm = 0.0;
    if (v < 0.7) {
        vm = mix(1.0, 0.5, clamp(v / 0.7, 0.0, 1.0));
    } else {
        vm = mix(0.5, 0.0, clamp((v - 0.7) / 0.3, 0.0, 1.0));
    }
    return a * (1.0 - abs(2.0 * u - 1.0)) * vm;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(screen_texture));
    let base = textureSampleLevel(screen_texture, texture_sampler, in.uv, 0.0);
    var col = base.rgb;

    let w = band_weight(in.uv.y);
    if (w > 0.002) {
        // golden-angle disc blur, radius growing towards the screen edge
        let r = params.band.z * (dims.y / 720.0) * (0.35 + 0.65 * w);
        var acc = base.rgb;
        var n = 1.0;
        for (var k = 0; k < TAPS; k = k + 1) {
            let fk = f32(k) + 0.5;
            let rr = r * sqrt(fk / f32(TAPS));
            let ang = fk * GOLDEN;
            let off = vec2<f32>(cos(ang), sin(ang)) * rr / dims;
            acc = acc + textureSampleLevel(screen_texture, texture_sampler, in.uv + off, 0.0).rgb;
            n = n + 1.0;
        }
        col = mix(col, acc / n, clamp(w, 0.0, 1.0));
    }

    if (params.shafts.x > 0.05) {
        let px = in.uv * dims;
        var s = 0.0;
        for (var i = 0; i < 5; i = i + 1) {
            s = s + shaft(px, dims, f32(i));
        }
        col = col + params.shaft_color.rgb * s;
    }
    return vec4<f32>(col, 1.0);
}

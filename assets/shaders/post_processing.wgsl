@group(0) @binding(0)
var post_process_texture: texture_2d<f32>;

@group(0) @binding(1)
var post_process_sampler: sampler;

@group(0) @binding(2)
var<uniform> settings: PostProcessSettings;

struct PostProcessSettings {
    intensity: f32,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(3.0, 1.0),
        vec2<f32>(-1.0, 1.0),
    );

    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 2.0),
        vec2<f32>(2.0, 0.0),
        vec2<f32>(0.0, 0.0),
    );

    var out: VertexOutput;
    out.position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    out.uv = uvs[vertex_index];
    return out;
}

@fragment
fn fragment(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let center = vec2<f32>(0.5, 0.5);
    let offset = uv - center;
    let dist = length(offset);
    
    // Apply distortion: push pixels outward or inward
    let factor = 1.0 + settings.intensity * dist * dist;
    let distorted_uv = center + offset * factor;

    // Optional: clamp to avoid sampling outside
    if any(distorted_uv < vec2<f32>(0.0)) || any(distorted_uv > vec2<f32>(1.0)) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    return textureSample(post_process_texture, post_process_sampler, distorted_uv);
}
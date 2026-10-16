// Glyph sprite shader for text rendering
// Renders instanced quads at precise pixel positions

struct Uniforms {
    screen_size: vec2<f32>,
    cell_size: vec2<f32>,
    grid_offset: vec2<f32>,
    grid_dims: vec2<u32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(0) @binding(1)
var glyph_texture: texture_2d<f32>;

@group(0) @binding(2)
var glyph_sampler: sampler;

@group(0) @binding(3)
var color_texture: texture_2d<f32>;

// Per-instance glyph data
struct GlyphInstance {
    @location(0) pos: vec2<f32>,      // Pixel position (x, y)
    @location(1) size: vec2<f32>,     // Size in pixels (w, h)
    @location(2) uv_pos: vec2<f32>,   // Atlas UV origin
    @location(3) uv_size: vec2<f32>,  // Atlas UV size
    @location(4) color: vec4<f32>,    // Text color RGBA
    @location(5) flags: f32,          // 1.0: RGBA color glyph, 0.0: mask
    @location(6) clip: vec4<f32>,     // Clip rect (x0, y0, x1, y1)
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) flags: f32,
    @location(3) clip_pos: vec2<f32>, // Pixel position for clipping
    @location(4) clip: vec4<f32>,
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_idx: u32,
    instance: GlyphInstance,
) -> VertexOutput {
    // Quad vertices (two triangles)
    var quad = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 1.0),
    );

    let local = quad[vertex_idx];

    // Pixel position
    let pixel_pos = instance.pos + local * instance.size;

    // Convert to NDC
    let ndc = vec2<f32>(
        (pixel_pos.x / uniforms.screen_size.x) * 2.0 - 1.0,
        1.0 - (pixel_pos.y / uniforms.screen_size.y) * 2.0
    );

    var out: VertexOutput;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = instance.uv_pos + local * instance.uv_size;
    out.color = instance.color;
    out.flags = instance.flags;
    out.clip_pos = pixel_pos;
    out.clip = instance.clip;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // No row paints outside its text region: fragments outside the
    // instance's clip rectangle are discarded, whatever the shaper's
    // natural width.
    if (in.clip_pos.x < in.clip.x || in.clip_pos.x >= in.clip.z
        || in.clip_pos.y < in.clip.y || in.clip_pos.y >= in.clip.w) {
        discard;
    }
    if (in.flags > 0.5) {
        // Color-font glyph (CBDT/COLR emoji): the atlas holds straight
        // RGBA, painted verbatim; only the text alpha still applies.
        let texel = textureSample(color_texture, glyph_sampler, in.uv);
        return vec4<f32>(texel.rgb, texel.a * in.color.a);
    }
    let alpha = textureSample(glyph_texture, glyph_sampler, in.uv).r;
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}

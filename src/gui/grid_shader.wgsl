// Grid rendering shader for era GUI
// Renders colored cells with optional text glyphs

// Uniforms
struct Uniforms {
    screen_size: vec2<f32>,
    cell_size: vec2<f32>,
    grid_offset: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

// Per-instance cell data
struct CellInstance {
    @location(0) cell_pos: vec2<f32>,    // Grid position (column, row)
    @location(1) fg_color: vec4<f32>,    // Foreground color
    @location(2) bg_color: vec4<f32>,    // Background color
    @location(3) glyph_uv: vec4<f32>,    // UV coords in atlas (x, y, w, h)
    @location(4) glyph_metrics: vec4<f32>, // Reserved for future use
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) fg_color: vec4<f32>,
    @location(1) bg_color: vec4<f32>,
    @location(2) local_uv: vec2<f32>,    // 0-1 within cell
    @location(3) glyph_uv: vec4<f32>,
}

// Vertex shader - generates a quad for each cell
@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    instance: CellInstance,
) -> VertexOutput {
    // Generate quad vertices (0-5 for two triangles)
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 1.0),
    );
    
    let local_pos = positions[vertex_index];
    
    // Calculate pixel position
    let cell_pixel_pos = uniforms.grid_offset + instance.cell_pos * uniforms.cell_size;
    let pixel_pos = cell_pixel_pos + local_pos * uniforms.cell_size;
    
    // Convert to NDC (-1 to 1)
    let ndc = vec2<f32>(
        (pixel_pos.x / uniforms.screen_size.x) * 2.0 - 1.0,
        1.0 - (pixel_pos.y / uniforms.screen_size.y) * 2.0  // Flip Y
    );
    
    var output: VertexOutput;
    output.position = vec4<f32>(ndc, 0.0, 1.0);
    output.fg_color = instance.fg_color;
    output.bg_color = instance.bg_color;
    output.local_uv = local_pos;
    output.glyph_uv = instance.glyph_uv;
    return output;
}

// Glyph texture atlas
@group(0) @binding(1)
var glyph_texture: texture_2d<f32>;
@group(0) @binding(2)
var glyph_sampler: sampler;

// Fragment shader - renders cell background and optional glyph
@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Start with background color
    var color = input.bg_color;
    
    // If we have a glyph (non-zero UV size), sample and blend
    if (input.glyph_uv.z > 0.0 && input.glyph_uv.w > 0.0) {
        // Calculate UV in atlas
        let atlas_uv = input.glyph_uv.xy + input.local_uv * input.glyph_uv.zw;
        let glyph_alpha = textureSample(glyph_texture, glyph_sampler, atlas_uv).r;
        
        // Blend foreground color with background based on glyph alpha
        color = mix(color, input.fg_color, glyph_alpha);
    }
    
    return color;
}

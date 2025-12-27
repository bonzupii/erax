// Background shader for gap-free cell rendering
// Uses storage buffer to read cell colors directly
// Calculates cell index from fragment coordinates

struct Uniforms {
    screen_size: vec2<f32>,
    cell_size: vec2<f32>,
    grid_offset: vec2<f32>,
    grid_dims: vec2<u32>,  // (columns, rows)
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(0) @binding(1)
var<storage, read> bg_colors: array<u32>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
}

// Fullscreen triangle
@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOutput {
    // Generate fullscreen triangle
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    
    var out: VertexOutput;
    out.position = vec4<f32>(positions[idx], 0.0, 1.0);
    return out;
}

// Unpack RGBA from u32 (ABGR format)
fn unpack_color(packed: u32) -> vec4<f32> {
    let r = f32((packed >> 0u) & 0xFFu) / 255.0;
    let g = f32((packed >> 8u) & 0xFFu) / 255.0;
    let b = f32((packed >> 16u) & 0xFFu) / 255.0;
    let a = f32((packed >> 24u) & 0xFFu) / 255.0;
    return vec4<f32>(r, g, b, a);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Convert from pixel coordinates to grid coordinates
    let pixel = in.position.xy - uniforms.grid_offset;
    
    // Check if we're outside the grid area
    if (pixel.x < 0.0 || pixel.y < 0.0) {
        return vec4<f32>(0.1, 0.1, 0.12, 1.0); // Background color
    }
    
    let cell = vec2<u32>(
        u32(floor(pixel.x / uniforms.cell_size.x)),
        u32(floor(pixel.y / uniforms.cell_size.y))
    );
    
    // Check bounds
    if (cell.x >= uniforms.grid_dims.x || cell.y >= uniforms.grid_dims.y) {
        return vec4<f32>(0.1, 0.1, 0.12, 1.0); // Background color
    }
    
    let idx = cell.y * uniforms.grid_dims.x + cell.x;
    let packed = bg_colors[idx];
    
    return unpack_color(packed);
}

// Background shader for gap-free cell rendering.
//
// One u32 texture carries the packed cell background colors, a second the
// cell semantic roles (`CellRole` discriminants, src/terminal/model.rs);
// textures work on WebGL2, where fragment shaders cannot use storage
// buffers. The shader indexes both by fragment position.
//
// Two theme facts arrive as uniforms: `bg` (the theme background) paints
// out-of-grid fragments, and the effect magnitudes drive the presentation
// treatments. Every treatment is a luminance step of the cell's own
// background, signed on the CPU from the theme's darkness, so a zeroed
// uniform is exactly the identity.

struct Uniforms {
    screen_size: vec2<f32>,
    cell_size: vec2<f32>,
    grid_offset: vec2<f32>,
    grid_dims: vec2<u32>,  // (columns, rows)
    bg: vec4<f32>,         // theme background for out-of-grid fragments
    panel_shift: f32,      // panel-role luminance step (0 = off)
    panel_edge: f32,       // panel run edge luminance step (0 = off)
    selection_edge: f32,   // selection run top-edge step (0 = off)
    cursor_halo: f32,      // cursor-neighbor blend factor (0 = off)
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(0) @binding(1)
var bg_tex: texture_2d<u32>;

@group(0) @binding(2)
var role_tex: texture_2d<u32>;

// CellRole discriminants (src/terminal/model.rs, repr(u8)). The renderer
// uploads `role as u32` per cell; a headless test pins both sides.
const ROLE_EDITOR_TEXT: u32 = 0u;
const ROLE_GUTTER: u32 = 1u;
const ROLE_STATUS_BAR: u32 = 2u;
const ROLE_MENU_BAR: u32 = 3u;
const ROLE_MENU_ITEM: u32 = 4u;
const ROLE_PROMPT: u32 = 5u;
const ROLE_MESSAGE_LINE: u32 = 6u;
const ROLE_CURSOR: u32 = 7u;
const ROLE_SELECTION: u32 = 8u;
const ROLE_SCROLLBAR: u32 = 9u;
const ROLE_DIAGNOSTIC: u32 = 10u;
const ROLE_SEPARATOR: u32 = 11u;

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

fn is_panel_role(role: u32) -> bool {
    return role == ROLE_STATUS_BAR || role == ROLE_MENU_BAR || role == ROLE_MENU_ITEM
        || role == ROLE_PROMPT || role == ROLE_MESSAGE_LINE;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Convert from pixel coordinates to grid coordinates
    let pixel = in.position.xy - uniforms.grid_offset;

    // Outside the grid the theme background shows.
    if (pixel.x < 0.0 || pixel.y < 0.0) {
        return uniforms.bg;
    }

    let cell = vec2<u32>(
        u32(floor(pixel.x / uniforms.cell_size.x)),
        u32(floor(pixel.y / uniforms.cell_size.y))
    );

    // Check bounds
    if (cell.x >= uniforms.grid_dims.x || cell.y >= uniforms.grid_dims.y) {
        return uniforms.bg;
    }

    // Textures hold one u32 per cell (R32Uint); textureLoad needs no sampler.
    let coord = vec2<i32>(cell);
    var color = unpack_color(textureLoad(bg_tex, coord, 0).x);
    let role = textureLoad(role_tex, coord, 0).x;

    // Panel depth: one uniform luminance step for panel roles.
    if (is_panel_role(role)) {
        color.rgb += vec3<f32>(uniforms.panel_shift);
    }

    // Neighbor roles for run-edge detection. An out-of-grid neighbor reads
    // as the cell's own role, so no edge paints at the screen boundary.
    var above_role = role;
    if (cell.y > 0u) {
        above_role = textureLoad(role_tex, coord + vec2<i32>(0, -1), 0).x;
    }
    var below_role = role;
    if (cell.y + 1u < uniforms.grid_dims.y) {
        below_role = textureLoad(role_tex, coord + vec2<i32>(0, 1), 0).x;
    }

    // Panel run edges: a stronger luminance step on the first and last rows
    // of each panel run. The check is per cell column, so a dropdown floating
    // over editor text gets edges exactly where the roles change.
    if (is_panel_role(role)) {
        let row_top = f32(cell.y) * uniforms.cell_size.y;
        if (above_role != role && pixel.y - row_top < 1.0) {
            color.rgb += vec3<f32>(uniforms.panel_edge);
        }
        let row_bottom = row_top + uniforms.cell_size.y;
        if (below_role != role && row_bottom - pixel.y < 1.0) {
            color.rgb += vec3<f32>(uniforms.panel_edge);
        }
    }

    // Selection top edge: the first row of a selection run gets a defined
    // upper boundary beyond the theme's selection tint.
    if (role == ROLE_SELECTION && above_role != ROLE_SELECTION) {
        let row_top = f32(cell.y) * uniforms.cell_size.y;
        if (pixel.y - row_top < 1.0) {
            color.rgb += vec3<f32>(uniforms.selection_edge);
        }
    }

    // Cursor halo: cells neighboring the cursor blend toward the cursor
    // cell's background. Exactly one cursor cell exists per frame.
    if (uniforms.cursor_halo > 0.0 && role != ROLE_CURSOR) {
        let offsets = array<vec2<i32>, 4>(
            vec2<i32>(1, 0), vec2<i32>(-1, 0),
            vec2<i32>(0, 1), vec2<i32>(0, -1),
        );
        for (var i = 0; i < 4; i++) {
            let n = coord + offsets[i];
            if (n.x >= 0 && n.y >= 0
                && u32(n.x) < uniforms.grid_dims.x
                && u32(n.y) < uniforms.grid_dims.y
                && textureLoad(role_tex, n, 0).x == ROLE_CURSOR) {
                let halo = unpack_color(textureLoad(bg_tex, n, 0).x).rgb;
                color.rgb = mix(color.rgb, halo, uniforms.cursor_halo);
                break;
            }
        }
    }

    return color;
}

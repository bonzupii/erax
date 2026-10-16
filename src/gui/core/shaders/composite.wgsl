// Chrome compositing pass: cell backgrounds plus GUI chrome.
//
// One RGBA8 texture carries the per-cell background colors (filterable,
// so the glass blur kernel samples smoothly); a second R32Uint texture
// carries the cell semantic roles (`CellRole` discriminants,
// src/terminal/model.rs). Textures work on WebGL2, where fragment
// shaders cannot use storage buffers. The shader indexes both by
// fragment position.
//
// For panel roles (menu bar, dropdown items, prompt, message line,
// status bar) the pass renders the chrome treatment selected by the
// uniforms: frosted glass (a blur kernel over the cell-background
// grid, tinted toward the theme background), a specular edge
// highlight, and — for floating panels — a drop shadow cast onto the
// surrounding cells. The Solid style reproduces the legacy opaque
// panel lift and run edge through the same uniforms. Every other cell
// paints its own background sharp; with all chrome uniforms zeroed
// the pass is exactly the identity over the cell colors.
//
// There is no "behind" in the grid model: a dropdown's cells replace
// the text cells, so the blur works the cell-background grid, not
// hidden content. The panel text itself is drawn sharp by the glyph
// pass after this one.

struct Uniforms {
    screen_size: vec2<f32>,
    cell_size: vec2<f32>,
    grid_offset: vec2<f32>,
    grid_dims: vec2<u32>,  // (columns, rows)
    bg: vec4<f32>,         // theme background for out-of-grid fragments
    glass: f32,            // frosted panel backgrounds (0/1)
    blur_radius_px: f32,   // glass blur kernel radius (0 = tint only)
    glass_tint: f32,       // mix of the blurred fill toward the theme bg
    specular: f32,         // specular edge highlight strength (0 = off)
    shadow_strength: f32,  // drop shadow strength (0 = off)
    shadow_px: f32,        // drop shadow reach in pixels
    panel_shift: f32,      // Solid-style signed panel lift (0 = off)
    panel_edge: f32,       // Solid-style signed panel run edge (0 = off)
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(0) @binding(1)
var bg_tex: texture_2d<f32>;

@group(0) @binding(2)
var bg_sampler: sampler;

@group(0) @binding(3)
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
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );

    var out: VertexOutput;
    out.position = vec4<f32>(positions[idx], 0.0, 1.0);
    return out;
}

fn is_panel_role(role: u32) -> bool {
    return role == ROLE_STATUS_BAR || role == ROLE_MENU_BAR || role == ROLE_MENU_ITEM
        || role == ROLE_PROMPT || role == ROLE_MESSAGE_LINE;
}

fn is_floating_panel_role(role: u32) -> bool {
    return role == ROLE_MENU_ITEM;
}

// 13-tap gaussian-ish blur over the cell-background grid, in pixel
// space. `bg_tex` filters bilinearly, so taps between cell centers
// interpolate smoothly. Weights sum to 1.0.
fn blur_bg(cell_f: vec2<f32>) -> vec3<f32> {
    let dims = vec2<f32>(uniforms.grid_dims);
    let uv = (cell_f + vec2<f32>(0.5)) / dims;
    let r = uniforms.blur_radius_px;
    // Pixel offsets converted to bg_tex UV units.
    let px = vec2<f32>(r / uniforms.cell_size.x / dims.x, r / uniforms.cell_size.y / dims.y);
    let hx = vec2<f32>(px.x * 0.5, 0.0);
    let hy = vec2<f32>(0.0, px.y * 0.5);
    let dg = vec2<f32>(px.x * 0.7071, px.y * 0.7071);
    var acc = textureSample(bg_tex, bg_sampler, uv).rgb * 0.16;
    acc = acc + textureSample(bg_tex, bg_sampler, uv + vec2<f32>(px.x, 0.0)).rgb * 0.1;
    acc = acc + textureSample(bg_tex, bg_sampler, uv - vec2<f32>(px.x, 0.0)).rgb * 0.1;
    acc = acc + textureSample(bg_tex, bg_sampler, uv + vec2<f32>(0.0, px.y)).rgb * 0.1;
    acc = acc + textureSample(bg_tex, bg_sampler, uv - vec2<f32>(0.0, px.y)).rgb * 0.1;
    acc = acc + textureSample(bg_tex, bg_sampler, uv + hx).rgb * 0.06;
    acc = acc + textureSample(bg_tex, bg_sampler, uv - hx).rgb * 0.06;
    acc = acc + textureSample(bg_tex, bg_sampler, uv + hy).rgb * 0.06;
    acc = acc + textureSample(bg_tex, bg_sampler, uv - hy).rgb * 0.06;
    acc = acc + textureSample(bg_tex, bg_sampler, uv + dg).rgb * 0.05;
    acc = acc + textureSample(bg_tex, bg_sampler, uv + vec2<f32>(-dg.x, dg.y)).rgb * 0.05;
    acc = acc + textureSample(bg_tex, bg_sampler, uv + vec2<f32>(dg.x, -dg.y)).rgb * 0.05;
    acc = acc + textureSample(bg_tex, bg_sampler, uv - dg).rgb * 0.05;
    return acc;
}

// Role of the neighboring cell, or `0xFFFFFFFFu` when out of grid.
// Out-of-grid counts as "not a panel role": a docked bar's screen edge
// is a boundary and gets its specular highlight.
fn neighbor_role(coord: vec2<i32>, dx: i32, dy: i32) -> u32 {
    let n = coord + vec2<i32>(dx, dy);
    if (n.x < 0 || n.y < 0
        || u32(n.x) >= uniforms.grid_dims.x
        || u32(n.y) >= uniforms.grid_dims.y) {
        return 0xFFFFFFFFu;
    }
    return textureLoad(role_tex, n, 0).x;
}

// Y of the nearest floating-panel bottom edge in the fragment's
// column, or -1 if none within the walk. The walk covers 4 cells,
// which honors the 32px config bound at cell heights down to 8px;
// the default 10px never leaves the first cell.
fn panel_bottom_above(coord: vec2<i32>, cell_y: u32, cell_f: vec2<f32>) -> f32 {
    for (var dy = 1; dy <= 4; dy++) {
        if (u32(dy) > cell_y) {
            return -1.0;
        }
        if (is_floating_panel_role(neighbor_role(coord, 0, -dy))) {
            // The edge is this panel cell's bottom only when the cell
            // below it is not a floating panel; for dy=1 that is the
            // fragment's own cell, non-panel by construction.
            var below_is_panel = false;
            if (dy > 1) {
                below_is_panel =
                    is_floating_panel_role(neighbor_role(coord, 0, 1 - dy));
            }
            if (!below_is_panel) {
                return (cell_f.y - f32(dy) + 1.0) * uniforms.cell_size.y;
            }
        }
    }
    return -1.0;
}

// X of the nearest floating-panel right edge in the fragment's row,
// or -1 if none within the walk. Mirrors `panel_bottom_above`.
fn panel_right_left_of(coord: vec2<i32>, cell_x: u32, cell_f: vec2<f32>) -> f32 {
    for (var dx = 1; dx <= 4; dx++) {
        if (u32(dx) > cell_x) {
            return -1.0;
        }
        if (is_floating_panel_role(neighbor_role(coord, -dx, 0))) {
            var right_is_panel = false;
            if (dx > 1) {
                right_is_panel =
                    is_floating_panel_role(neighbor_role(coord, 1 - dx, 0));
            }
            if (!right_is_panel) {
                return (cell_f.x - f32(dx) + 1.0) * uniforms.cell_size.x;
            }
        }
    }
    return -1.0;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Convert from pixel coordinates to grid coordinates.
    let pixel = in.position.xy - uniforms.grid_offset;

    // Outside the grid the theme background shows.
    if (pixel.x < 0.0 || pixel.y < 0.0) {
        return uniforms.bg;
    }

    let cell_f = vec2<f32>(
        floor(pixel.x / uniforms.cell_size.x),
        floor(pixel.y / uniforms.cell_size.y)
    );
    let cell = vec2<u32>(u32(cell_f.x), u32(cell_f.y));

    // Check bounds
    if (cell.x >= uniforms.grid_dims.x || cell.y >= uniforms.grid_dims.y) {
        return uniforms.bg;
    }

    // bg_tex is Rgba8Unorm: textureLoad returns normalized floats, no
    // unpacking. role_tex is R32Uint, one CellRole discriminant per cell.
    let coord = vec2<i32>(cell);
    let cell_bg = textureLoad(bg_tex, coord, 0);
    let role = textureLoad(role_tex, coord, 0).x;

    if (!is_panel_role(role)) {
        // Not chrome: the cell's own background, sharp. A drop shadow
        // darkens the surroundings of floating panels with a linear
        // falloff; the glyph pass draws text sharp over it. The corner
        // diagonal is not shadowed (a one-cell cosmetic gap); the
        // below/right walks cover the configured reach.
        var rgb = cell_bg.rgb;
        if (uniforms.shadow_strength > 0.0 && uniforms.shadow_px > 0.0) {
            // Below a floating panel's bottom edge.
            let bottom = panel_bottom_above(coord, cell.y, cell_f);
            if (bottom >= 0.0) {
                let d = pixel.y - bottom;
                if (d >= 0.0 && d < uniforms.shadow_px) {
                    let f = 1.0 - uniforms.shadow_strength * (1.0 - d / uniforms.shadow_px);
                    rgb = rgb * f;
                }
            }
            // Right of a floating panel's right edge.
            let right = panel_right_left_of(coord, cell.x, cell_f);
            if (right >= 0.0) {
                let d = pixel.x - right;
                if (d >= 0.0 && d < uniforms.shadow_px) {
                    let f = 1.0 - uniforms.shadow_strength * (1.0 - d / uniforms.shadow_px);
                    rgb = rgb * f;
                }
            }
        }
        return vec4<f32>(rgb, cell_bg.a);
    }

    // Panel chrome.
    var prgb: vec3<f32>;
    if (uniforms.glass > 0.5) {
        var frosted = cell_bg.rgb;
        if (uniforms.blur_radius_px > 0.0) {
            frosted = blur_bg(cell_f);
        }
        // Glass tint: pull the frosted fill toward the theme background.
        prgb = mix(frosted, uniforms.bg.rgb, uniforms.glass_tint);
    } else {
        prgb = cell_bg.rgb;
    }

    // Solid-style legacy panel lift (signed on the CPU from the theme's
    // darkness, so the shader stays theme-agnostic).
    prgb = prgb + vec3<f32>(uniforms.panel_shift);

    // Solid-style run edge: a stronger luminance step on the first and
    // last rows of each panel run, per cell column. An out-of-grid
    // neighbor reads as the cell's own role, so no edge paints at the
    // screen boundary. The gate is `!= 0.0` (not `> 0.0`): the magnitude
    // is CPU-signed from the theme's darkness, and the old `> 0.0` gate
    // silently dropped the edge on light themes.
    if (uniforms.panel_edge != 0.0) {
        var above_role = role;
        var below_role = role;
        if (cell.y > 0u) {
            above_role = textureLoad(role_tex, coord + vec2<i32>(0, -1), 0).x;
        }
        if (cell.y + 1u < uniforms.grid_dims.y) {
            below_role = textureLoad(role_tex, coord + vec2<i32>(0, 1), 0).x;
        }
        let row_top = cell_f.y * uniforms.cell_size.y;
        if (above_role != role && pixel.y - row_top < 1.0) {
            prgb = prgb + vec3<f32>(uniforms.panel_edge);
        }
        let row_bottom = row_top + uniforms.cell_size.y;
        if (below_role != role && row_bottom - pixel.y < 1.0) {
            prgb = prgb + vec3<f32>(uniforms.panel_edge);
        }
    }

    // Specular highlight: a 1px always-add band on every panel
    // boundary, screen edges included (a docked bar's top edge is the
    // classic glass highlight).
    if (uniforms.specular > 0.0) {
        let left = cell_f.x * uniforms.cell_size.x;
        let top = cell_f.y * uniforms.cell_size.y;
        let right = left + uniforms.cell_size.x;
        let bottom = top + uniforms.cell_size.y;
        var hi = vec3<f32>(0.0);
        if (!is_panel_role(neighbor_role(coord, -1, 0)) && pixel.x - left < 1.0) {
            hi = hi + vec3<f32>(uniforms.specular);
        }
        if (!is_panel_role(neighbor_role(coord, 1, 0)) && right - pixel.x < 1.0) {
            hi = hi + vec3<f32>(uniforms.specular);
        }
        if (!is_panel_role(neighbor_role(coord, 0, -1)) && pixel.y - top < 1.0) {
            hi = hi + vec3<f32>(uniforms.specular);
        }
        if (!is_panel_role(neighbor_role(coord, 0, 1)) && bottom - pixel.y < 1.0) {
            hi = hi + vec3<f32>(uniforms.specular);
        }
        prgb = prgb + hi;
    }

    return vec4<f32>(prgb, cell_bg.a);
}

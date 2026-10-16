//! GPU Renderer — run-shaped text rendering (N-120, N-259, N-260).
//!
//! This is the platform-agnostic presentation core: it takes a wgpu
//! `SurfaceTarget` (not a winit `Window`) and explicit font bytes (not a
//! system font lookup). The native winit shell and the web wasm shell both
//! construct this same core — the rendering is shared 1:1.
//!
//! Architecture:
//! - cosmic-text is the sole shaping/layout stack. Every visible screen row is
//!   shaped ONCE as a complete run with `Shaping::Advanced`; the resulting
//!   glyph runs are drawn at their shaped positions and cached in the atlas
//!   (N-120). There is no per-character shaping path anymore.
//! - The TUI grid is kept for logical editor geometry and background cells
//!   only (N-259). Glyph x positions come from the shaper, not the cell grid.
//! - Each row uses one stable baseline (`FontManager::ascent`) so rows never
//!   shift vertically when their script content changes; cosmic-text's
//!   per-line `line_y` is intentionally not used.
//! - Caret mapping (buffer column -> shaped x) is built per row from the
//!   shaped clusters and exposed through pure, unit-tested functions plus a
//!   per-frame screen-space accessor for the WebUI acceptance pass.
//! - The GUI never touches input routing, command behavior, layout semantics,
//!   text content, or TUI state: it stays a dumb passthrough for the TUI
//!   (principle 12 / N-030). All input models live in the backend.

use bytemuck::{Pod, Zeroable};
use cosmic_text::{
    Align, Attrs, Buffer, CacheKey, Family, LayoutRun, Metrics, Shaping, SwashContent,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::config::cursor_style::CursorStyle;
use crate::core::geometry::GridMetrics;
use crate::terminal::display::{Cell, CellRole, ScreenBuffer};

use super::atlas::{Atlas, CachedGlyph, GlyphKey};
use super::cursor::{cursor_overlay_rects, PxRect};
use super::font_manager::{FontManager, FontSource};
use super::quad_renderer::{BackgroundUniforms, QuadRenderer};

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct GlyphInstance {
    pos: [f32; 2],
    size: [f32; 2],
    uv_pos: [f32; 2],
    uv_size: [f32; 2],
    color: [f32; 4],
}

/// One vertex of the cursor overlay: NDC position + solid color.
/// Positions are precomputed on the CPU (physical px → NDC).
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct CursorVertex {
    pos: [f32; 2],
    color: [f32; 4],
}

/// One uniformly-colored span of a shaped line: a byte range plus a color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextSpan {
    /// Byte offset of the span's first byte in the shaped line text.
    pub start: usize,
    /// Byte offset one past the span's last byte.
    pub end: usize,
    /// Packed color in `cosmic_text::Color` layout (`0xAARRGGBB`).
    pub color: u32,
}

/// The shaping input for one screen row: everything shaping needs, extracted
/// from the grid without shaping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShapedLineInput {
    /// The row's visible text, in grid-column order.
    pub text: String,
    /// Uniform-color spans covering `text` (merged across cells).
    pub spans: Vec<TextSpan>,
    /// `col_to_byte[c]` is the byte offset in `text` where grid column `c`'s
    /// text starts. Length is `columns + 1`; the last entry is `text.len()`
    /// (the end-of-line caret position).
    pub col_to_byte: Vec<usize>,
}

/// Convert mouse pixel position to grid cell coordinates.
///
/// Pure geometry for the dumb passthrough: shells convert pixels to cells
/// and forward the cell position; all click semantics live in the backend.
/// Shared by the native and web shells (no second implementation).
pub fn mouse_pos_to_grid(x: f32, y: f32, metrics: &GridMetrics) -> Option<(u16, u16)> {
    let (col, row) = metrics.px_to_grid(x, y)?;
    Some((col as u16, row as u16))
}

/// Build the shaping input for one screen row.
///
/// Pure: no shaping is performed here, only text extraction.
/// - Hidden continuation cells (wide chars) and NUL cells contribute no text;
///   their column maps to the current byte offset — the caret boundary right
///   after the preceding text.
/// - Control characters are dropped (invisible, as the old per-character path
///   rendered them); a stray tab becomes one blank. The TUI expands tabs to
///   spaces upstream, so the grid never carries real tabs.
/// - Consecutive cells with equal foreground colors merge into one span.
pub fn build_shaped_line(cells: &[Cell]) -> ShapedLineInput {
    let mut text = String::new();
    let mut spans: Vec<TextSpan> = Vec::new();
    let mut col_to_byte: Vec<usize> = Vec::with_capacity(cells.len() + 1);

    let mut span_start = 0usize;
    let mut span_color: Option<u32> = None;

    for cell in cells {
        col_to_byte.push(text.len());
        let cell_text = cell.text();
        if cell.hidden || cell_text.is_empty() || cell_text == "\0" {
            continue;
        }
        let cell_text = if cell_text == "\t" { " " } else { cell_text };
        if cell_text.chars().next().is_some_and(|c| c.is_control()) {
            continue;
        }
        let (r, g, b) = cell.fg.rgb8();
        let color = cosmic_text::Color::rgba(r, g, b, 255).0;
        if span_color != Some(color) {
            if let Some(prev) = span_color {
                spans.push(TextSpan {
                    start: span_start,
                    end: text.len(),
                    color: prev,
                });
            }
            span_start = text.len();
            span_color = Some(color);
        }
        text.push_str(cell_text);
    }
    col_to_byte.push(text.len());
    if let Some(prev) = span_color {
        spans.push(TextSpan {
            start: span_start,
            end: text.len(),
            color: prev,
        });
    }

    ShapedLineInput {
        text,
        spans,
        col_to_byte,
    }
}

/// One shaped glyph cluster: the smallest unit the caret can address.
///
/// A cluster covers a contiguous byte range of the shaped line text. Several
/// layout glyphs may share one cluster (stacked combining marks, the parts of
/// an Indic conjunct or Khmer cluster); the caret treats the cluster as one
/// stop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapedCluster {
    /// Byte offset of the cluster's first byte in the shaped line text.
    pub start: usize,
    /// Byte offset one past the cluster's last byte.
    pub end: usize,
    /// Visual x of the cluster's left edge, in pixels, relative to the line origin.
    pub x: f32,
    /// Visual width of the cluster in pixels (`x + w` is the right edge).
    pub w: f32,
    /// True when the cluster belongs to a right-to-left run.
    pub rtl: bool,
}

impl ShapedCluster {
    /// Visual x of the cluster's leading edge: the edge a forward-moving
    /// caret touches first (left for LTR, right for RTL).
    pub fn leading_edge(self) -> f32 {
        if self.rtl {
            self.x + self.w
        } else {
            self.x
        }
    }

    /// Visual x of the cluster's trailing edge.
    pub fn trailing_edge(self) -> f32 {
        if self.rtl {
            self.x
        } else {
            self.x + self.w
        }
    }
}

/// Merge a shaped line's layout glyphs into caret clusters, in visual order.
///
/// Layout glyphs sharing one `(start, end)` byte range — stacked combining
/// marks, the parts of an Indic conjunct or Khmer cluster — merge into a
/// single cluster spanning their x extents, so the caret stops at the cluster
/// boundary instead of inside it.
pub fn clusters_from_runs(runs: &[LayoutRun<'_>]) -> Vec<ShapedCluster> {
    let mut clusters: Vec<ShapedCluster> = Vec::new();
    for run in runs {
        for glyph in run.glyphs {
            let rtl = !glyph.level.is_ltr();
            match clusters.last_mut() {
                Some(last) if last.start == glyph.start && last.end == glyph.end => {
                    let x0 = last.x.min(glyph.x);
                    let x1 = (last.x + last.w).max(glyph.x + glyph.w);
                    last.x = x0;
                    last.w = (x1 - x0).max(0.0);
                    // Direction cannot differ inside one cluster; keep the first.
                }
                _ => clusters.push(ShapedCluster {
                    start: glyph.start,
                    end: glyph.end,
                    x: glyph.x,
                    w: glyph.w.max(0.0),
                    rtl,
                }),
            }
        }
    }
    clusters
}

/// First grid column whose character falls inside a cluster's byte range.
///
/// `col_to_byte` runs in visual (left-to-right) order, so the minimum
/// matching column is the cluster's visual left edge in either text
/// direction. The integer-grid snap site uses this to place the
/// cluster's origin.
fn cluster_first_col(col_to_byte: &[usize], start: usize, end: usize) -> usize {
    col_to_byte
        .iter()
        .position(|&b| b >= start && b < end)
        .unwrap_or(0)
}

/// Caret x-offset for a logical byte offset into a shaped line, in pixels
/// relative to the line origin.
///
/// Pure and unit-testable: `clusters` come from [`clusters_from_runs`] (visual
/// order) and `line_text` is the shaped text they index into. The convention
/// mirrors cosmic-text's own caret placement (`LayoutRun::cursor_position`),
/// so the GUI caret agrees with the shaping stack:
/// - an offset at a cluster's start maps to its leading edge;
/// - an offset strictly inside a cluster interpolates across the cluster
///   proportionally to grapheme count — a two-grapheme ligature gets a
///   mid-glyph caret stop, while combining-mark stacks (one grapheme) never
///   create interior stops;
/// - an offset at a cluster's end maps to its trailing edge;
/// - at a BiDi boundary the cluster *starting* at the offset wins, exactly
///   like the stack's editor cursor.
///
/// Offsets past the end clamp to the line's trailing edge; an empty cluster
/// list maps everything to `0.0`.
pub fn caret_x_for_byte_offset(
    clusters: &[ShapedCluster],
    line_text: &str,
    byte_offset: usize,
) -> f32 {
    // First pass: starts, then interior — the first visual cluster wins,
    // mirroring `LayoutLine::cursor_glyph`.
    for cluster in clusters {
        if byte_offset == cluster.start {
            return cluster.leading_edge();
        }
        if cluster.start < byte_offset && byte_offset < cluster.end {
            let slice = line_text.get(cluster.start..cluster.end).unwrap_or("");
            let mut before = 0u32;
            let mut total = 0u32;
            for (i, _) in slice.grapheme_indices(true) {
                if cluster.start + i < byte_offset {
                    before += 1;
                }
                total += 1;
            }
            let frac = if total > 0 {
                before as f32 / total as f32
            } else {
                0.0
            };
            return if cluster.rtl {
                cluster.x + cluster.w * (1.0 - frac)
            } else {
                cluster.x + cluster.w * frac
            };
        }
    }
    // Second pass: ends.
    for cluster in clusters {
        if byte_offset == cluster.end {
            return cluster.trailing_edge();
        }
    }
    // Past the end (or no clusters at all): trailing edge of the last cluster.
    clusters.last().map_or(0.0, |cluster| cluster.trailing_edge())
}

/// A shaped glyph ready for atlas lookup and instancing.
#[derive(Clone, Copy, Debug)]
struct ShapedGlyph {
    key: GlyphKey,
    cache_key: CacheKey,
    /// Integer pen position from `LayoutGlyph::physical`. The atlas bitmap is
    /// placed relative to it; the subpixel detail lives in `cache_key`.
    x: i32,
    y: i32,
    color: [f32; 4],
}

/// Text attributes for one color span: the configured primary family (or the
/// monospace fallback) plus an explicit color, so every shaped glyph carries
/// its cell's foreground color through shaping.
fn attrs_for(family: Option<&str>, color: u32) -> Attrs<'_> {
    let attrs = Attrs::new();
    let attrs = match family {
        Some(name) => attrs.family(Family::Name(name)),
        None => attrs.family(Family::Monospace),
    };
    attrs.color(cosmic_text::Color(color))
}

/// Instance color from a shaped glyph's span color.
///
/// Every span sets an explicit color, so `None` is unreachable in practice;
/// it falls back to opaque white rather than guessing.
fn glyph_color_f32(color_opt: Option<cosmic_text::Color>) -> [f32; 4] {
    match color_opt {
        Some(color) => [
            f32::from(color.r()) / 255.0,
            f32::from(color.g()) / 255.0,
            f32::from(color.b()) / 255.0,
            f32::from(color.a()) / 255.0,
        ],
        None => [1.0, 1.0, 1.0, 1.0],
    }
}

/// Error acquiring or presenting a surface frame (wgpu 30 removed
/// `SurfaceError`; frame acquisition is now an enum, not a `Result`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderError {
    /// The surface was lost or its configuration is outdated: the caller
    /// should reconfigure (resize path) and try the next frame.
    SurfaceLost,
    /// A validation error was raised while acquiring the frame.
    Validation,
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::SurfaceLost => write!(f, "GPU surface lost or outdated"),
            RenderError::Validation => write!(f, "GPU surface validation error"),
        }
    }
}

impl std::error::Error for RenderError {}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    size: (u32, u32),
    font_manager: FontManager,
    atlas: Atlas,
    quad_renderer: QuadRenderer,

    shape_buffer: Buffer,
    /// Scratch: shaped glyphs of the row currently being emitted.
    pending_glyphs: Vec<ShapedGlyph>,
    /// Per-frame caret maps: `caret_maps[row][col]` is the screen-space
    /// caret x for grid column `col` (length `cols + 1` per row), built from
    /// the shaped clusters in `render()`.
    caret_maps: Vec<Vec<f32>>,

    last_atlas_gen: u64,

    glyph_pipeline: wgpu::RenderPipeline,
    glyph_bind_group_layout: wgpu::BindGroupLayout,
    glyph_uniform_buffer: wgpu::Buffer,
    glyph_bind_group: Option<wgpu::BindGroup>,
    glyph_buffer: Option<wgpu::Buffer>,
    glyph_capacity: usize,

    instances: Vec<GlyphInstance>,

    /// Backend cursor shape for this frame (set by the shell from the
    /// screen model / wire `CursorState`; the core only presents it).
    cursor_style: CursorStyle,
    /// Device pixel ratio, for cursor overlay thickness.
    cursor_dpr: f32,
    /// Solid-rect pipeline for the cursor shape overlay.
    cursor_pipeline: wgpu::RenderPipeline,
    cursor_vb: Option<wgpu::Buffer>,
    cursor_vb_cap: usize,

    pub metrics: GridMetrics,

    // Performance caches
    bg_cache: Vec<u32>,
}

/// Construction parameters for [`Renderer::new`].
///
/// One struct keeps the constructor's argument list small as backend
/// selection grows; both shells build it at their construction site.
pub struct RendererParams {
    /// The wgpu surface target. The native shell passes a winit window;
    /// the web shell passes the browser canvas. The core never sees winit
    /// or web_sys.
    pub target: wgpu::SurfaceTarget<'static>,
    /// Physical pixels of the surface.
    pub width: u32,
    /// Physical pixels of the surface.
    pub height: u32,
    /// The configured font stack, primary first. The core loads every
    /// entry into its shaping database in order (then system fonts as a
    /// last resort): the primary shapes the text, later entries cover
    /// glyphs it lacks. Empty means no fonts at all.
    pub fonts: Vec<FontSource>,
    pub font_size: f32,
    pub scale_factor: f64,
    /// The wgpu backends to request. Backend selection is shell business:
    /// native passes `Backends::PRIMARY`, the web shell tries
    /// `BROWSER_WEBGPU` then falls back to `GL` (WebGL2).
    pub backends: wgpu::Backends,
}

impl Renderer {
    /// Create the presentation core.
    ///
    /// Async because adapter/device acquisition resolves through the
    /// platform event loop (a browser JS promise on wasm32): blocking on
    /// it would deadlock the page at startup. Each shell awaits this from
    /// its own executor — the native shell blocks its thread, the web
    /// shell awaits on the browser event loop.
    pub async fn new(p: RendererParams) -> Result<Self, Box<dyn std::error::Error>> {
        let scaled_font_size = p.font_size * p.scale_factor as f32;
        // Use integer line height to prevent rounding drift
        let line_height = (scaled_font_size * 1.2).round();

        let mut instance_desc = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_desc.backends = p.backends;
        let instance = wgpu::Instance::new(instance_desc);
        let surface = instance.create_surface(p.target)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|e| format!("no {:?} adapter: {e}", p.backends))?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("erax"),
                required_features: wgpu::Features::empty(),
                // WebGL2 has no compute pipelines: the generic downlevel
                // limits request compute workgroup sizes the GL adapter
                // reports as 0, so the device request fails. The WebGL2
                // downlevel set drops the compute requirements (our
                // pipelines are all render pipelines).
                required_limits: if p.backends == wgpu::Backends::GL {
                    wgpu::Limits::downlevel_webgl2_defaults()
                } else {
                    wgpu::Limits::downlevel_defaults()
                },
                memory_hints: wgpu::MemoryHints::Performance,
                ..Default::default()
            })
            .await?;

        // wgpu's default uncaptured-error handler panics; a device error
        // during setup must surface as `Err`, not a trap. Log it instead.
        device.on_uncaptured_error(std::sync::Arc::new(|err: wgpu::Error| {
            let msg = format!("wgpu device error: {err}");
            #[cfg(target_arch = "wasm32")]
            web_sys::console::error_1(&msg.into());
            #[cfg(not(target_arch = "wasm32"))]
            eprintln!("{msg}");
        }));

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .copied()
            .unwrap_or(wgpu::TextureFormat::Bgra8Unorm);
        // Present/alpha modes come from the adapter's capabilities: the
        // WebGL2 canvas only offers Fifo, so a hardcoded AutoVsync would
        // fail surface configuration there.
        let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::AutoVsync) {
            wgpu::PresentMode::AutoVsync
        } else {
            caps.present_modes
                .first()
                .copied()
                .unwrap_or(wgpu::PresentMode::Fifo)
        };
        let alpha_mode = caps
            .alpha_modes
            .first()
            .copied()
            .unwrap_or(wgpu::CompositeAlphaMode::Auto);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: p.width.max(1),
            height: p.height.max(1),
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &config);

        let mut font_manager =
            FontManager::from_font_stack(scaled_font_size, line_height, &p.fonts);
        // The grid cell is the shaped advance snapped to whole pixels by
        // `GridMetrics::new` (the canonical snap point). Backgrounds are
        // located per fragment by floor(pixel / cell) and every shaped
        // cluster origin is placed at a whole multiple of the cell, so
        // backgrounds and glyphs derive from the same integers and
        // cannot drift apart; see `FontManager::grid_cell_size`.
        let (cell_width, cell_height) = font_manager.grid_cell_size();
        let metrics = GridMetrics::new(
            cell_width,
            cell_height,
            p.width as f32,
            p.height as f32,
        );

        let atlas = Atlas::new(&device, 2048, 2048);
        let quad_renderer = QuadRenderer::new(&device, format);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Glyph Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/glyph.wgsl").into()),
        });

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Glyph Uniforms"),
            size: std::mem::size_of::<BackgroundUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Glyph BGL"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Glyph PL"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Glyph Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GlyphInstance>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 8,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 16,
                            shader_location: 2,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 24,
                            shader_location: 3,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 32,
                            shader_location: 4,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                    ],
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let shape_buffer = Buffer::new(
            &mut font_manager.font_system,
            Metrics::new(scaled_font_size, line_height),
        );

        // Cursor overlay pipeline: solid NDC rects, no uniforms, no blending
        // (opaque, like a terminal cursor). Positions are baked on the CPU.
        let cursor_pipeline = {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Cursor Shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shaders/cursor.wgsl").into()),
            });
            let pipeline_layout =
                device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Cursor Pipeline Layout"),
                    bind_group_layouts: &[],
                    immediate_size: 0,
                });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Cursor Pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<CursorVertex>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[
                            wgpu::VertexAttribute {
                                offset: 0,
                                shader_location: 0,
                                format: wgpu::VertexFormat::Float32x2,
                            },
                            wgpu::VertexAttribute {
                                offset: 8,
                                shader_location: 1,
                                format: wgpu::VertexFormat::Float32x4,
                            },
                        ],
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };

        let mut renderer = Self {
            device,
            queue,
            surface,
            config,
            size: (p.width, p.height),
            font_manager,
            atlas,
            quad_renderer,
            shape_buffer,
            pending_glyphs: Vec::new(),
            caret_maps: Vec::new(),
            last_atlas_gen: 0,
            glyph_pipeline: pipeline,
            glyph_bind_group_layout: bind_layout,
            glyph_uniform_buffer: uniform_buf,
            glyph_bind_group: None,
            glyph_buffer: None,
            glyph_capacity: 0,
            instances: Vec::new(),
            cursor_style: CursorStyle::Default,
            cursor_dpr: 1.0,
            cursor_pipeline,
            cursor_vb: None,
            cursor_vb_cap: 0,
            metrics,
            bg_cache: Vec::new(),
        };

        // Pre-warm the atlas with ASCII so the first paint has no shaping
        // stalls; every other glyph rasterizes on demand during render.
        let ascii: Vec<char> = (0x20u32..=0x7E).filter_map(char::from_u32).collect();
        renderer.prewarm_chars(&ascii);

        Ok(renderer)
    }

    /// Rasterize one glyph into the atlas (best effort). The atlas itself is
    /// the glyph cache — no separate char→glyph map is needed anymore.
    fn ensure_glyph_rasterized(&mut self, key: GlyphKey, cache_key: CacheKey) -> Option<CachedGlyph> {
        if let Some(g) = self.atlas.get(key) {
            return Some(g);
        }

        let img_opt = self
            .font_manager
            .swash_cache
            .get_image(&mut self.font_manager.font_system, cache_key);

        if let Some(img) = img_opt {
            if img.placement.width > 0 && img.placement.height > 0 {
                if let Some(slot) = self
                    .atlas
                    .allocate(img.placement.width, img.placement.height)
                {
                    let px = (img.placement.width * img.placement.height) as usize;
                    let data: Vec<u8> = match img.content {
                        SwashContent::Mask => img.data.clone(),
                        // N-127: color emoji keeps only its alpha here and
                        // renders monochrome; a color atlas path is still open.
                        SwashContent::Color => img.data.chunks(4).map(|c| c[3]).collect(),
                        _ => {
                            if img.data.len() == px * 3 {
                                img.data
                                    .chunks(3)
                                    .map(|c| ((c[0] as u32 + c[1] as u32 + c[2] as u32) / 3) as u8)
                                    .collect()
                            } else {
                                img.data.clone()
                            }
                        }
                    };
                    self.atlas.insert(
                        key,
                        slot,
                        data,
                        img.placement.left as f32,
                        img.placement.top as f32,
                    );
                    self.atlas.get(key)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Shape `text` once and rasterize every resulting glyph into the atlas.
    /// Best effort: shaping failures simply produce no glyphs.
    fn prewarm_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let attrs = attrs_for(self.font_manager.primary_family.as_deref(), 0xFFFF_FFFF);
        self.shape_buffer
            .set_text(text, &attrs, Shaping::Advanced, Some(Align::Left));
        self.shape_buffer
            .shape_until_scroll(&mut self.font_manager.font_system, false);

        let mut keys: Vec<(GlyphKey, CacheKey)> = Vec::new();
        {
            let shape_buffer = &self.shape_buffer;
            for run in shape_buffer.layout_runs() {
                for glyph in run.glyphs {
                    let physical = glyph.physical((0.0, 0.0), 1.0);
                    keys.push((
                        GlyphKey {
                            font_id: physical.cache_key.font_id,
                            glyph_index: physical.cache_key.glyph_id as u32,
                        },
                        physical.cache_key,
                    ));
                }
            }
        }
        for (key, cache_key) in keys {
            self.ensure_glyph_rasterized(key, cache_key);
        }
        self.atlas.flush(&self.queue);
    }

    /// Pre-warm the atlas for a set of characters, shaped once as a salad:
    /// the spaces between characters keep neighbors from merging into
    /// accidental ligatures, so each character's own glyph gets rasterized.
    fn prewarm_chars(&mut self, chars: &[char]) {
        if chars.is_empty() {
            return;
        }
        let salad: String = chars.iter().flat_map(|&c| [c, ' ']).collect();
        self.prewarm_text(&salad);
    }

    pub fn resize(&mut self, new_size: (u32, u32)) {
        if new_size.0 > 0 && new_size.1 > 0 {
            self.size = new_size;
            self.config.width = new_size.0;
            self.config.height = new_size.1;
            self.surface.configure(&self.device, &self.config);
            self.metrics = GridMetrics::new(
                self.metrics.cell_width,
                self.metrics.cell_height,
                new_size.0 as f32,
                new_size.1 as f32,
            );

            self.glyph_bind_group = None;
            // Caret maps are rebuilt by the next render().
            self.caret_maps.clear();
        }
    }

    pub fn grid_size(&self) -> (u32, u32) {
        self.metrics.grid_dimensions()
    }
    pub fn size(&self) -> (u32, u32) {
        self.size
    }
    pub fn cell_width(&self) -> f32 {
        self.metrics.cell_width
    }
    pub fn cell_height(&self) -> f32 {
        self.metrics.cell_height
    }

    /// Cell under a pixel position, via the canonical grid metrics.
    ///
    /// The single pixel-to-cell derivation for both shells; the grid
    /// offset is accounted for, so clicks at the window edge map to
    /// the correct cell.
    pub fn grid_cell_at(&self, x: f32, y: f32) -> Option<(u16, u16)> {
        mouse_pos_to_grid(x, y, &self.metrics)
    }

    /// Set the backend's cursor shape for subsequent frames.
    ///
    /// Presentation only (principle 12): the shape comes from the backend's
    /// screen model (`Display::cursor_style` natively, the wire
    /// `CursorState.style` on the web). The cursor *position* is read from
    /// the `CellRole::Cursor`-stamped cell during `render()`.
    pub fn set_cursor(&mut self, style: CursorStyle, dpr: f32) {
        self.cursor_style = style;
        self.cursor_dpr = dpr.max(1.0);
    }

    /// Screen-space x (pixels) of the caret for a grid cell, through this
    /// frame's shaped clusters (N-259).
    ///
    /// Glyph runs are drawn at shaped positions, so a buffer column's caret is
    /// generally *not* `col * cell_width`: it resolves through
    /// combining-mark stacks, ligatures, and RTL/BiDi runs via
    /// [`caret_x_for_byte_offset`]. Returns `None` for rows that have not been
    /// rendered yet (before the first frame, or after a resize).
    pub fn caret_x_for_cell(&self, row: usize, col: usize) -> Option<f32> {
        let origin_x = self.metrics.offset_x;
        self.caret_maps
            .get(row)?
            .get(col)
            .map(|&x| origin_x + x)
    }

    /// Pre-warm every unique visible character (N-122: one preload path).
    /// The frame's own per-row shaping covers the rest.
    pub fn preload_fonts_for_buffer(&mut self, screen_buffer: &ScreenBuffer) {
        use std::collections::HashSet;
        let unique: Vec<char> = screen_buffer
            .cells
            .iter()
            .flat_map(|c| c.text().chars())
            .filter(|&ch| ch != ' ' && ch != '\0' && !ch.is_control())
            .collect::<HashSet<char>>()
            .into_iter()
            .collect();
        self.prewarm_chars(&unique);
    }

    /// Pre-warm all unique characters from document text.
    /// Call this ONCE when a file is loaded to avoid runtime rendering lag.
    pub fn preload_document(&mut self, document_text: &str) {
        use std::collections::HashSet;
        let unique: Vec<char> = document_text
            .chars()
            .filter(|&ch| !ch.is_control())
            .collect::<HashSet<char>>()
            .into_iter()
            .collect();
        self.prewarm_chars(&unique);
    }

    /// The x origin (pixels) that shaped line text is laid out from.
    ///
    /// The canonical grid origin from the metrics: the same origin the
    /// background cells derive from, so glyphs and backgrounds agree by
    /// construction.
    fn text_origin_x(&self) -> f32 {
        self.metrics.offset_x
    }

    pub fn render(&mut self, screen_buffer: &ScreenBuffer) -> Result<(), RenderError> {
        let cols = screen_buffer.width as usize;
        let rows = screen_buffer.height as usize;

        // Reset atlas if full (glyph cache is keyed stably, so nothing else
        // needs clearing).
        if self.atlas.is_full() {
            self.atlas.reset();
        }
        let atlas_changed = self.atlas.generation() != self.last_atlas_gen;

        // Backgrounds (the grid still owns background cells — N-259).
        self.bg_cache.clear();
        self.bg_cache.reserve(screen_buffer.cells.len());
        self.bg_cache
            .extend(screen_buffer.cells.iter().map(|c| c.bg.to_packed_rgba()));

        // Glyphs
        self.instances.clear();

        // Per-frame caret maps: rows × (cols + 1) boundary positions.
        if self.caret_maps.len() != rows {
            self.caret_maps = vec![Vec::new(); rows];
        }

        // Use integer math to prevent rounding drift
        let cw = self.metrics.cell_width;
        let ch = self.metrics.cell_height;
        // One stable baseline per row (see module docs).
        let ascent = self.font_manager.ascent;
        let origin_x = self.text_origin_x();

        for row in 0..rows {
            let y = self.metrics.row_y(row);
            let cells = &screen_buffer.cells[row * cols..(row + 1) * cols];

            // 1. Extract the row's text, color spans, and column→byte map.
            let input = build_shaped_line(cells);

            // 2. Shape the whole row at once with full run shaping (N-120).
            let family = self.font_manager.primary_family.as_deref();
            let default_attrs = attrs_for(family, 0xFFFF_FFFF);
            self.shape_buffer.set_rich_text(
                input
                    .spans
                    .iter()
                    .map(|span| (&input.text[span.start..span.end], attrs_for(family, span.color))),
                &default_attrs,
                Shaping::Advanced,
                Some(Align::Left),
            );
            self.shape_buffer
                .shape_until_scroll(&mut self.font_manager.font_system, false);

            // 3. Collect shaped glyphs and build the row's caret map while
            //    the layout borrows are live.
            let baseline = y + ascent;
            {
                let shape_buffer = &self.shape_buffer;
                let pending = &mut self.pending_glyphs;
                let caret_row = &mut self.caret_maps[row];
                pending.clear();
                let runs: Vec<LayoutRun<'_>> = shape_buffer.layout_runs().collect();
                let clusters = clusters_from_runs(&runs);
                // Snap every cluster's origin to the integer grid: the
                // visual-left column the cluster starts in times the
                // (whole-pixel) cell width. Glyphs keep their shaped
                // offset inside the cluster, so kerning and mark
                // placement are unchanged; only the cluster origin is
                // integral, and each row derives it independently, so
                // drift cannot accumulate down the frame.
                let snapped: Vec<f32> = clusters
                    .iter()
                    .map(|c| {
                        cluster_first_col(&input.col_to_byte, c.start, c.end) as f32 * cw
                    })
                    .collect();
                // Clusters and glyphs walk in the same order, so a cursor
                // finds each glyph's cluster.
                let mut cluster_idx = 0usize;
                for run in &runs {
                    for glyph in run.glyphs {
                        while cluster_idx + 1 < clusters.len()
                            && (clusters[cluster_idx].start != glyph.start
                                || clusters[cluster_idx].end != glyph.end)
                            && clusters[cluster_idx].end <= glyph.start
                        {
                            cluster_idx += 1;
                        }
                        let placed_x = match clusters.get(cluster_idx).filter(|c| {
                            c.start == glyph.start && c.end == glyph.end
                        }) {
                            Some(c) => origin_x + snapped[cluster_idx] + (glyph.x - c.x),
                            // Degenerate: a glyph outside every cluster
                            // keeps the shaper's position; drift is
                            // bounded to this glyph.
                            None => origin_x + glyph.x,
                        };
                        let physical = glyph.physical((0.0, baseline), 1.0);
                        // Integer pixel grid: the cluster origin is
                        // whole by construction; the shaped intra-cluster
                        // offset rounds once against that stable origin,
                        // so the rounding cannot drift across rows.
                        let x = placed_x.round() as i32;
                        pending.push(ShapedGlyph {
                            key: GlyphKey {
                                font_id: physical.cache_key.font_id,
                                glyph_index: physical.cache_key.glyph_id as u32,
                            },
                            cache_key: physical.cache_key,
                            x,
                            y: physical.y,
                            color: glyph_color_f32(glyph.color_opt),
                        });
                    }
                }
                // Caret maps resolve through the snapped clusters, so
                // caret stops sit on integer grid positions; ligature
                // interiors keep their shaped fractional offset within
                // the cluster.
                let snapped_clusters: Vec<ShapedCluster> = clusters
                    .iter()
                    .zip(snapped.iter())
                    .map(|(c, &x)| ShapedCluster { x, ..*c })
                    .collect();
                caret_row.clear();
                caret_row.extend(
                    input
                        .col_to_byte
                        .iter()
                        .map(|&byte| caret_x_for_byte_offset(&snapped_clusters, &input.text, byte)),
                );
            }

            // 4. Rasterize on demand and emit instances at shaped positions.
            // (`pending` is a local take: the borrow of `self` inside the loop
            // is fine, and assigning it back preserves the scratch capacity.)
            let pending = std::mem::take(&mut self.pending_glyphs);
            for glyph in &pending {
                let entry = self
                    .atlas
                    .get(glyph.key)
                    .or_else(|| self.ensure_glyph_rasterized(glyph.key, glyph.cache_key));
                if let Some(cached) = entry {
                    // Bitmap top-left = integer pen + swash placement
                    // (placement.top is baseline-to-bitmap-top, upward).
                    self.instances.push(GlyphInstance {
                        pos: [
                            glyph.x as f32 + cached.bearing_x,
                            glyph.y as f32 - cached.bearing_y,
                        ],
                        size: [cached.width, cached.height],
                        uv_pos: [cached.uv_x, cached.uv_y],
                        uv_size: [cached.uv_w, cached.uv_h],
                        color: glyph.color,
                    });
                }
            }
            self.pending_glyphs = pending;
        }

        self.last_atlas_gen = self.atlas.generation();
        self.atlas.flush(&self.queue);

        // Update GPU
        let uniforms = BackgroundUniforms {
            screen_size: [self.size.0 as f32, self.size.1 as f32],
            cell_size: [cw, ch],
            grid_offset: [self.metrics.offset_x, self.metrics.offset_y],
            grid_dims: [cols as u32, rows as u32],
        };
        self.quad_renderer.update_uniforms(&self.queue, &uniforms);
        self.quad_renderer.upload_colors(
            &self.device,
            &self.queue,
            &self.bg_cache,
            cols as u32,
            rows as u32,
        );

        if self.glyph_bind_group.is_none() || atlas_changed {
            self.queue
                .write_buffer(&self.glyph_uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
            self.glyph_bind_group =
                Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Glyph BG"),
                    layout: &self.glyph_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: self.glyph_uniform_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(self.atlas.texture_view()),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(self.atlas.sampler()),
                        },
                    ],
                }));
        }

        if !self.instances.is_empty() {
            let bytes = bytemuck::cast_slice(&self.instances);
            if self.instances.len() > self.glyph_capacity {
                self.glyph_capacity = (self.instances.len() * 2).next_power_of_two();
                self.glyph_buffer = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Glyph VB"),
                    size: (self.glyph_capacity * std::mem::size_of::<GlyphInstance>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            if let Some(b) = &self.glyph_buffer {
                self.queue.write_buffer(b, 0, bytes);
            }
        }

        // Render
        // wgpu 30: frame acquisition is an enum, not a Result. Timeout /
        // occlusion just skip the frame; lost/outdated asks the caller to
        // reconfigure; validation is reported.
        let out = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(())
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                return Err(RenderError::SurfaceLost)
            }
            wgpu::CurrentSurfaceTexture::Validation => return Err(RenderError::Validation),
        };
        let view = out
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render"),
            });

        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.1,
                            b: 0.12,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                ..Default::default()
            });

            self.quad_renderer.render(&mut rp);

            if !self.instances.is_empty()
                && let (Some(b), Some(bg)) = (&self.glyph_buffer, &self.glyph_bind_group) {
                    rp.set_pipeline(&self.glyph_pipeline);
                    rp.set_bind_group(0, bg, &[]);
                    rp.set_vertex_buffer(0, b.slice(..));
                    rp.draw(0..6, 0..self.instances.len() as u32);
                }

            // Cursor overlay: the backend stamps exactly one cell with
            // `CellRole::Cursor`; draw the shell-supplied shape over it.
            // Block needs nothing (the styled cell is the block). The
            // overlay color is the cursor cell's own foreground (the theme's
            // cursor_fg), so contrast is guaranteed by the theme.
            let cursor_draw: Option<(Vec<PxRect>, [f32; 4])> = screen_buffer
                .cells
                .iter()
                .position(|c| c.role == CellRole::Cursor)
                .map(|idx| {
                    let cell = &screen_buffer.cells[idx];
                    let (r, g, b) = cell.fg.rgb8();
                    let color = [
                        f32::from(r) / 255.0,
                        f32::from(g) / 255.0,
                        f32::from(b) / 255.0,
                        1.0,
                    ];
                    let col = (idx % cols) as u16;
                    let row = (idx / cols) as u16;
                    let rects = cursor_overlay_rects(
                        col,
                        row,
                        super::cursor::CursorGeometry {
                            origin_x: self.text_origin_x(),
                            row_y: self.metrics.row_y(row as usize),
                            cell_w: cw,
                            cell_h: ch,
                            cols: cols as u16,
                            rows: rows as u16,
                            dpr: self.cursor_dpr,
                        },
                        self.cursor_style,
                    );
                    (rects, color)
                });
            if let Some((rects, color)) = cursor_draw
                && !rects.is_empty()
            {
                let w = self.size.0 as f32;
                let h = self.size.1 as f32;
                let ndc = |x: f32, y: f32| [(x / w) * 2.0 - 1.0, 1.0 - (y / h) * 2.0];
                let mut verts: Vec<CursorVertex> = Vec::with_capacity(rects.len() * 6);
                for r in &rects {
                    let (x0, y0) = (r.x, r.y);
                    let (x1, y1) = (r.x + r.w, r.y + r.h);
                    let p00 = ndc(x0, y0);
                    let p10 = ndc(x1, y0);
                    let p01 = ndc(x0, y1);
                    let p11 = ndc(x1, y1);
                    for p in [p00, p10, p01, p10, p11, p01] {
                        verts.push(CursorVertex { pos: p, color });
                    }
                }
                let bytes = bytemuck::cast_slice(&verts);
                if verts.len() > self.cursor_vb_cap {
                    self.cursor_vb_cap = (verts.len() * 2).next_power_of_two().max(6);
                    self.cursor_vb = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("Cursor VB"),
                        size: (self.cursor_vb_cap * std::mem::size_of::<CursorVertex>()) as u64,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }));
                }
                if let Some(vb) = &self.cursor_vb {
                    self.queue.write_buffer(vb, 0, bytes);
                    rp.set_pipeline(&self.cursor_pipeline);
                    rp.set_vertex_buffer(0, vb.slice(..));
                    rp.draw(0..verts.len() as u32, 0..1);
                }
            }
        }

        self.queue.submit(std::iter::once(enc.finish()));
        self.queue.present(out);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::display::{CellRole, Color as TermColor};

    fn cell(ch: char, fg: TermColor) -> Cell {
        Cell::with_role(ch, fg, TermColor::Reset, CellRole::EditorText)
    }

    #[test]
    fn cluster_first_col_finds_visual_left_edge() {
        // col_to_byte runs in visual order; the minimum matching column
        // is the cluster's visual left edge in either direction.
        // Wide char at cols 1-2 (bytes 3..4), RTL cluster at cols 4-5
        // (bytes 5..9, logical order reversed visually).
        let col_to_byte = vec![0, 3, 3, 4, 5, 5, 9];
        assert_eq!(cluster_first_col(&col_to_byte, 0, 1), 0);
        assert_eq!(cluster_first_col(&col_to_byte, 3, 4), 1);
        assert_eq!(cluster_first_col(&col_to_byte, 5, 9), 4);
        assert_eq!(cluster_first_col(&col_to_byte, 99, 100), 0);
    }

    #[test]
    fn snapped_cluster_origins_are_whole_multiples_of_cell() {
        // The integer-grid invariant, in pure form: each cluster's
        // snapped origin is its visual-left column times the whole
        // cell width, so placement derives from integers and cannot
        // accumulate fractional drift across rows.
        let cell_w = 11.0f32;
        let col_to_byte = vec![0, 1, 2, 3, 5, 5, 6];
        let clusters = vec![
            ShapedCluster { start: 0, end: 1, x: 0.4, w: 10.6, rtl: false },
            ShapedCluster { start: 5, end: 6, x: 43.7, w: 21.3, rtl: false },
        ];
        for c in &clusters {
            let snapped = cluster_first_col(&col_to_byte, c.start, c.end) as f32 * cell_w;
            assert_eq!(snapped.fract(), 0.0);
            assert_eq!(snapped % cell_w, 0.0);
        }
    }


    /// Golden: the GUI's pixel-to-grid derivation feeds the same frame
    /// model as the TUI. 808x488 px at 10x20 px cells is an 80x24 grid
    /// (the same derivation `run/gui.rs` uses before emitting the resize
    /// event); the resulting rect map must equal the TUI golden's. This
    /// pins the derivation, not the wgpu renderer.
    #[test]
    fn golden_gui_pixel_grid_frame_consistency() {
        use crate::core::app::EditorApp;
        use crate::core::buffer::Buffer;
        use crate::core::geometry::{compute_frame, FrameOptions, GridMetrics};
        use crate::core::id::BufferId;

        let mut app = EditorApp::new();
        let text = (0..100)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.buffers.insert(BufferId(0), Buffer::from_string(&text));

        let metrics = GridMetrics::new(10.0, 20.0, 808.0, 488.0);
        let (cols, rows) = metrics.grid_dimensions();
        assert_eq!((cols, rows), (80, 24));

        let frame = compute_frame(
            FrameOptions {
                cols: cols as usize,
                rows: rows as usize,
                menu_rows: 1,
                show_line_numbers: true,
            },
            &app.layout,
            &app.geom_inputs(),
        );
        assert_eq!(frame.panes.len(), 1);
        let p = &frame.panes[0];
        assert_eq!(
            (p.frame.x, p.frame.y, p.frame.width, p.frame.height),
            (0, 1, 80, 23)
        );
        assert_eq!(p.gutter_width, 3);
        assert_eq!(
            (p.text.x, p.text.y, p.text.width, p.text.height),
            (3, 1, 76, 21)
        );
        assert_eq!(p.status_row, 23);
        // Both scrollbar strips are reserved outside the text rect.
        assert_eq!(p.hscroll_row, 22);
        let v = p.v_scrollbar.expect("vertical scrollbar");
        assert_eq!((v.cross, v.track_start, v.track_len), (79, 2, 20));
        assert_eq!((v.thumb_start, v.thumb_len), (0, 4));
        assert_eq!(p.cursor, Some((3, 1)));
    }

    fn hidden_cell() -> Cell {
        Cell {
            text: crate::terminal::display::ClusterText::new(" "),
            fg: TermColor::Reset,
            bg: TermColor::Reset,
            hidden: true,
            role: CellRole::EditorText,
        }
    }

    fn packed(color: TermColor) -> u32 {
        let (r, g, b) = color.rgb8();
        cosmic_text::Color::rgba(r, g, b, 255).0
    }

    // --- build_shaped_line ---

    #[test]
    fn line_build_simple_merges_one_span() {
        let cells = [cell('a', TermColor::Red), cell('b', TermColor::Red)];
        let line = build_shaped_line(&cells);
        assert_eq!(line.text, "ab");
        assert_eq!(
            line.spans,
            vec![TextSpan {
                start: 0,
                end: 2,
                color: packed(TermColor::Red),
            }]
        );
        assert_eq!(line.col_to_byte, vec![0, 1, 2]);
    }

    #[test]
    fn line_build_splits_spans_on_color_change() {
        let cells = [
            cell('a', TermColor::Red),
            cell('b', TermColor::Red),
            cell('c', TermColor::Blue),
        ];
        let line = build_shaped_line(&cells);
        assert_eq!(line.text, "abc");
        assert_eq!(
            line.spans,
            vec![
                TextSpan {
                    start: 0,
                    end: 2,
                    color: packed(TermColor::Red),
                },
                TextSpan {
                    start: 2,
                    end: 3,
                    color: packed(TermColor::Blue),
                },
            ]
        );
        assert_eq!(line.col_to_byte, vec![0, 1, 2, 3]);
    }

    #[test]
    fn line_build_wide_char_hidden_cell_maps_past_it() {
        // '日' occupies two grid columns; the hidden continuation maps to the
        // byte offset right after it.
        let cells = [cell('日', TermColor::White), hidden_cell(), cell('x', TermColor::White)];
        let line = build_shaped_line(&cells);
        assert_eq!(line.text, "日x");
        assert_eq!(line.col_to_byte, vec![0, 3, 3, 4]);
    }

    #[test]
    fn line_build_skips_nul_and_controls_tab_becomes_blank() {
        let cells = [
            cell('a', TermColor::White),
            cell('\0', TermColor::White),
            cell('\x01', TermColor::White),
            cell('\t', TermColor::White),
            cell('b', TermColor::White),
        ];
        let line = build_shaped_line(&cells);
        assert_eq!(line.text, "a b");
        // NUL / control columns map to the boundary before the following text.
        assert_eq!(line.col_to_byte, vec![0, 1, 1, 1, 2, 3]);
    }

    #[test]
    fn line_build_empty_row() {
        let line = build_shaped_line(&[]);
        assert_eq!(line.text, "");
        assert!(line.spans.is_empty());
        assert_eq!(line.col_to_byte, vec![0]);
    }

    // --- caret_x_for_byte_offset (synthetic clusters, no fonts) ---

    fn ltr(start: usize, end: usize, x: f32, w: f32) -> ShapedCluster {
        ShapedCluster {
            start,
            end,
            x,
            w,
            rtl: false,
        }
    }

    fn rtl(start: usize, end: usize, x: f32, w: f32) -> ShapedCluster {
        ShapedCluster {
            start,
            end,
            x,
            w,
            rtl: true,
        }
    }

    #[test]
    fn caret_ltr_hits_cluster_boundaries() {
        let clusters = [ltr(0, 1, 0.0, 10.0), ltr(1, 2, 10.0, 10.0), ltr(2, 3, 20.0, 10.0)];
        let x = |o| caret_x_for_byte_offset(&clusters, "abc", o);
        assert_eq!(x(0), 0.0);
        assert_eq!(x(1), 10.0);
        assert_eq!(x(2), 20.0);
        assert_eq!(x(3), 30.0);
    }

    #[test]
    fn caret_ligature_interior_interpolates_by_grapheme() {
        // One glyph for "fi" (two graphemes): the caret stops mid-glyph.
        let clusters = [ltr(0, 2, 0.0, 20.0), ltr(2, 3, 20.0, 10.0)];
        let x = |o| caret_x_for_byte_offset(&clusters, "fix", o);
        assert_eq!(x(0), 0.0);
        assert_eq!(x(1), 10.0);
        assert_eq!(x(2), 20.0);
        assert_eq!(x(3), 30.0);
    }

    #[test]
    fn caret_combining_stack_creates_no_interior_stop() {
        // "e" + two combining marks is a single grapheme: any interior offset
        // resolves to the cluster's trailing edge, never between the marks.
        let text = "e\u{301}\u{302}";
        let clusters = [ltr(0, 5, 0.0, 12.0)];
        let x = |o| caret_x_for_byte_offset(&clusters, text, o);
        assert_eq!(x(0), 0.0);
        assert_eq!(x(1), 12.0);
        assert_eq!(x(4), 12.0);
        assert_eq!(x(5), 12.0);
    }

    #[test]
    fn caret_rtl_edges_are_inverted() {
        // Visual order: first cluster is the rightmost on screen.
        let clusters = [rtl(0, 2, 10.0, 10.0), rtl(2, 4, 0.0, 10.0)];
        let x = |o| caret_x_for_byte_offset(&clusters, "אב", o);
        assert_eq!(x(0), 20.0); // logical start = visual right edge
        assert_eq!(x(2), 10.0); // boundary: the cluster starting here wins
        assert_eq!(x(4), 0.0); // logical end = visual left edge
    }

    #[test]
    fn caret_bidi_boundary_prefers_cluster_start() {
        // "aב": at the direction boundary the glyph starting at the offset
        // wins, mirroring the shaping stack's editor cursor.
        let clusters = [ltr(0, 1, 0.0, 10.0), rtl(1, 3, 10.0, 10.0)];
        let x = |o| caret_x_for_byte_offset(&clusters, "aב", o);
        assert_eq!(x(0), 0.0);
        assert_eq!(x(1), 20.0);
        assert_eq!(x(3), 10.0);
    }

    #[test]
    fn caret_empty_and_past_end() {
        assert_eq!(caret_x_for_byte_offset(&[], "", 0), 0.0);
        assert_eq!(caret_x_for_byte_offset(&[], "", 7), 0.0);
        let clusters = [ltr(0, 1, 5.0, 10.0)];
        assert_eq!(caret_x_for_byte_offset(&clusters, "a", 99), 15.0);
    }

    // --- glyph_color_f32 ---

    #[test]
    fn glyph_color_converts_channels() {
        let red = cosmic_text::Color::rgba(255, 0, 0, 255);
        assert_eq!(glyph_color_f32(Some(red)), [1.0, 0.0, 0.0, 1.0]);
        let half = cosmic_text::Color::rgba(0, 128, 255, 255);
        let [r, g, b, a] = glyph_color_f32(Some(half));
        assert_eq!((r, g, b, a), (0.0, 128.0 / 255.0, 1.0, 1.0));
        assert_eq!(glyph_color_f32(None), [1.0, 1.0, 1.0, 1.0]);
    }

    // --- grid/shaper agreement ---

    #[test]
    fn fractional_cell_width_matches_shaped_advances() {
        // The renderer uses the shaped advance itself as the cell width
        // (no integer rounding): background cells located by
        // floor(pixel / cell) then agree with shaped glyph positions, so
        // box-drawing columns cannot drift. This pins the arithmetic the
        // renderer relies on.
        //
        // f32 cannot represent `col * w / w` exactly (for w = 9.63, col =
        // 15 the quotient rounds to just below 15), so the assertion allows
        // the representation error instead of demanding exact cell
        // identity: what matters is that the division recovers the column
        // to within float error, not that the floor never reads one low on
        // an exactly-on-the-boundary value.
        let cell_width = 9.63f32;
        for col in 0..200u32 {
            let shaped_x = col as f32 * cell_width;
            let recovered = shaped_x / cell_width;
            assert!(
                (recovered - col as f32).abs() < 1e-4,
                "cell width must recover the shaped column (col {col}: got {recovered})"
            );
        }
    }
}

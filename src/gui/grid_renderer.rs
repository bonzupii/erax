//! Complete GPU-accelerated grid renderer for era GUI mode
//!
//! This is a terminal emulator that renders the TUI's ScreenBuffer using wgpu.
//! Each cell is rendered as a colored quad with a glyph texture.

use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use bytemuck::{Pod, Zeroable};
use pollster::block_on;
use std::collections::HashMap;
use wgpu::util::DeviceExt;

use crate::core::geometry::GridMetrics;
use crate::terminal::display::ScreenBuffer;

/// Vertex data for instanced cell rendering
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct CellVertex {
    cell_pos: [f32; 2],
    fg_color: [f32; 4],
    bg_color: [f32; 4],
    glyph_uv: [f32; 4],      // UV coords in atlas: x, y, w, h
    glyph_metrics: [f32; 4], // Glyph metrics: offset_x, offset_y, width, height in pixels
}

/// Uniform buffer for grid rendering
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct GridUniforms {
    screen_size: [f32; 2],
    cell_size: [f32; 2],
    grid_offset: [f32; 2],
    _padding: [f32; 2],
}

/// Cached glyph information
struct GlyphInfo {
    // Atlas UV coordinates
    uv_x: f32,
    uv_y: f32,
    uv_w: f32,
    uv_h: f32,
    // Position within cell (in pixels)
    offset_x: f32,
    offset_y: f32,
    // Actual glyph size (in pixels)
    width: f32,
    height: f32,
}

/// Minimum window padding in pixels
const MIN_PADDING: f32 = 4.0;

/// GPU-accelerated terminal emulator renderer
pub struct GridRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    size: (u32, u32),

    // Grid rendering pipeline
    grid_pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,

    // Primary font and lazy-loaded fallbacks for missing glyphs
    font: FontVec,
    /// Names of fallback fonts to attempt loading (OS-specific priority order)
    fallback_font_names: Vec<&'static str>,
    /// Already loaded fallback fonts
    loaded_fallbacks: Vec<FontVec>,
    /// Index to next unloaded fallback name to try
    next_fallback_index: usize,
    font_scale: PxScale,
    pub cell_width: f32,
    pub cell_height: f32,

    // Glyph atlas texture
    glyph_atlas: wgpu::Texture,
    glyph_atlas_view: wgpu::TextureView,
    glyph_sampler: wgpu::Sampler,
    glyph_cache: HashMap<char, GlyphInfo>,
    atlas_width: u32,
    atlas_height: u32,
    atlas_cursor_x: u32,
    atlas_cursor_y: u32,

    // Uniform buffer
    uniform_buffer: wgpu::Buffer,

    // Instance buffer for cells (dynamically resized)
    instance_buffer: Option<wgpu::Buffer>,
    instance_capacity: usize,
}

impl GridRenderer {
    /// Create a new grid renderer
    /// scale_factor: window's scale factor for HiDPI support
    pub fn new(
        window: std::sync::Arc<winit::window::Window>,
        font_path: Option<&str>,
        font_size: f32,
        scale_factor: f64,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let size = window.inner_size();

        // Scale font size for HiDPI displays
        let scaled_font_size = font_size * scale_factor as f32;

        #[cfg(debug_assertions)]
        eprintln!(
            "GUI: scale_factor={:.2}, font_size={:.1}pt, scaled_font_size={:.1}pt, surface={}x{}",
            scale_factor, font_size, scaled_font_size, size.width, size.height
        );

        // Create wgpu instance - use PRIMARY backend for fastest init
        // primary = Vulkan (Linux), Metal (macOS), DX12 (Windows)
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        // Create surface safely using Arc<Window>
        let surface = instance.create_surface(window)?;

        // Request adapter - HighPerformance can be faster to find than LowPower
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .map_err(|_| "No suitable GPU adapter found")?;

        // Request device with minimal requirements for fastest init
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("erax device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(), // Less strict = faster
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::default(),
            trace: wgpu::Trace::default(),
        }))?;

        // Configure surface - prefer non-sRGB format to avoid double gamma correction
        // Theme colors are already in sRGB space, so we render directly without GPU gamma
        let surface_caps = surface.get_capabilities(&adapter);
        // Prefer Bgra8Unorm/Rgba8Unorm which are universally supported
        let surface_format = match surface_caps
            .formats
            .iter()
            .find(|f| {
                matches!(
                    f,
                    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
                )
            })
            .or_else(|| surface_caps.formats.iter().find(|f| f.is_srgb()))
            .copied()
        {
            Some(f) => f,
            None => surface_caps.formats[0],
        };

        // Use Immediate or Mailbox for faster first frame (no vsync wait)
        let present_mode = if surface_caps
            .present_modes
            .contains(&wgpu::PresentMode::Immediate)
        {
            wgpu::PresentMode::Immediate
        } else if surface_caps
            .present_modes
            .contains(&wgpu::PresentMode::Mailbox)
        {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::AutoVsync
        };

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 1, // Less latency = faster first frame
        };
        surface.configure(&device, &config);

        // Load font
        let font_data = Self::load_font(font_path)?;
        let font = FontVec::try_from_vec(font_data)?;
        let font_scale = PxScale::from(scaled_font_size);
        let scaled_font = font.as_scaled(font_scale);

        // Calculate cell dimensions - round to integers for pixel-perfect rendering
        // Use floor() for consistency: ensures cells never exceed the glyph advance
        // Box-drawing characters will fill exactly this space
        let glyph_id = font.glyph_id('M');
        let cell_width = scaled_font.h_advance(glyph_id).round();
        let cell_height = scaled_font.height().round();

        #[cfg(debug_assertions)]
        {
            let test_glyph = font.glyph_id('─');
            let box_advance = scaled_font.h_advance(test_glyph);
            eprintln!(
                "GUI: cell={}x{}, h_advance(M)={:.2}, h_advance(─)={:.2}",
                cell_width,
                cell_height,
                scaled_font.h_advance(glyph_id),
                box_advance
            );
        }

        // Create glyph atlas texture (2048x2048 = ~65k cells at 8x16)
        let atlas_width = 2048u32;
        let atlas_height = 2048u32;
        let glyph_atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Glyph Atlas"),
            size: wgpu::Extent3d {
                width: atlas_width,
                height: atlas_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let glyph_atlas_view = glyph_atlas.create_view(&wgpu::TextureViewDescriptor::default());
        let glyph_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Create bind group layout
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Grid Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
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
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
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

        // Create shader
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Grid Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("grid_shader.wgsl").into()),
        });

        // Create pipeline layout
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Grid Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        // Create render pipeline
        let grid_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Grid Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<CellVertex>() as wgpu::BufferAddress,
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
                            format: wgpu::VertexFormat::Float32x4,
                        },
                        wgpu::VertexAttribute {
                            offset: 24,
                            shader_location: 2,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                        wgpu::VertexAttribute {
                            offset: 40,
                            shader_location: 3,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                        wgpu::VertexAttribute {
                            offset: 56,
                            shader_location: 4,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                    ],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Create uniform buffer - will be updated on first resize
        let (cols, rows) = Self::compute_grid_dimensions(
            size.width as f32,
            size.height as f32,
            cell_width,
            cell_height,
        );
        let offset = Self::compute_centered_offset(
            size.width as f32,
            size.height as f32,
            cols,
            rows,
            cell_width,
            cell_height,
        );
        let uniforms = GridUniforms {
            screen_size: [size.width as f32, size.height as f32],
            cell_size: [cell_width, cell_height],
            grid_offset: offset,
            _padding: [0.0, 0.0],
        };
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Uniform Buffer"),
            contents: bytemuck::cast_slice(&[uniforms]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        Ok(Self {
            device,
            queue,
            surface,
            config,
            size: (size.width, size.height),
            grid_pipeline,
            bind_group_layout,
            font,
            fallback_font_names: Self::get_fallback_font_names(),
            loaded_fallbacks: Vec::new(),
            next_fallback_index: 0,
            font_scale,
            cell_width,
            cell_height,
            glyph_atlas,
            glyph_atlas_view,
            glyph_sampler,
            glyph_cache: HashMap::new(),
            atlas_width,
            atlas_height,
            atlas_cursor_x: 0,
            atlas_cursor_y: 0,
            uniform_buffer,
            instance_buffer: None,
            instance_capacity: 0,
        })
    }

    /// Load font from path or system font database using font-kit
    /// Uses proper system APIs: fontconfig (Linux), Core Text (macOS), DirectWrite (Windows)
    fn load_font(configured_font: Option<&str>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        use font_kit::family_name::FamilyName;
        use font_kit::properties::{Properties, Weight};
        use font_kit::source::SystemSource;

        // If user specified a font path, try loading it directly first (fast path)
        if let Some(path) = configured_font {
            if let Ok(data) = std::fs::read(path) {
                #[cfg(debug_assertions)]
                eprintln!("GUI: Using configured font path: {}", path);
                return Ok(data);
            }
        }

        // Create SystemSource ONCE - this is expensive on Linux (fontconfig init)
        let source = SystemSource::new();
        let mut props_builder = Properties::new();
        let props = props_builder.weight(Weight::NORMAL);

        // If user specified a font name, try it first
        if let Some(name) = configured_font {
            if let Ok(handle) =
                source.select_best_match(&[FamilyName::Title(name.to_string())], &props)
            {
                if let Ok(font) = handle.load() {
                    if let Some(data) = font.copy_font_data() {
                        #[cfg(debug_assertions)]
                        eprintln!("GUI: Using configured font: {}", name);
                        return Ok(data.to_vec());
                    }
                }
            }
        }

        // Try system monospace first (fastest - single fontconfig query)
        if let Ok(handle) = source.select_best_match(&[FamilyName::Monospace], &props) {
            if let Ok(font) = handle.load() {
                if let Some(data) = font.copy_font_data() {
                    #[cfg(debug_assertions)]
                    eprintln!("GUI: Using system default monospace");
                    return Ok(data.to_vec());
                }
            }
        }

        // Fallback: try common monospace fonts (reduced list for speed)
        let fallbacks = [
            "DejaVu Sans Mono", // Very common on Linux
            "Noto Sans Mono",
            "Liberation Mono",
            "Consolas", // Windows
            "Menlo",    // macOS
        ];

        for name in &fallbacks {
            if let Ok(handle) =
                source.select_best_match(&[FamilyName::Title(name.to_string())], &props)
            {
                if let Ok(font) = handle.load() {
                    if let Some(data) = font.copy_font_data() {
                        #[cfg(debug_assertions)]
                        eprintln!("GUI: Using fallback font: {}", name);
                        return Ok(data.to_vec());
                    }
                }
            }
        }

        Err("No monospace font found. Install a monospace font (e.g., noto-fonts-mono).".into())
    }

    /// Get list of fallback font names to try (lazy loaded on-demand)
    /// OS-specific fonts prioritized, then universal fallbacks
    fn get_fallback_font_names() -> Vec<&'static str> {
        let mut names: Vec<&'static str> = Vec::new();

        // OS-specific priority fonts first
        #[cfg(target_os = "macos")]
        {
            names.extend_from_slice(&[
                "Menlo",
                "Monaco",
                "Hiragino Sans",
                "Hiragino Kaku Gothic Pro",
                "Apple Color Emoji",
            ]);
        }

        #[cfg(target_os = "windows")]
        {
            names.extend_from_slice(&[
                "Consolas",
                "Cascadia Code",
                "MS Gothic",
                "MS Mincho",
                "Segoe UI Emoji",
                "Segoe UI Historic",
            ]);
        }

        #[cfg(target_os = "linux")]
        {
            names.extend_from_slice(&[
                "DejaVu Sans Mono",
                "Noto Sans Mono",
                "Noto Color Emoji",
                "WenQuanYi Micro Hei Mono",
            ]);
        }

        // Universal fallbacks (cross-platform)
        // Ordered by script importance and availability
        names.extend_from_slice(&[
            // === Wide Unicode Coverage ===
            "Noto Sans Mono",
            "DejaVu Sans Mono",
            "DejaVu Sans", // Has many scripts
            "Unifont",     // Massive coverage - prioritize
            "GNU Unifont",
            "FreeMono",
            "FreeSans",
            // === CJK ===
            "Noto Sans Mono CJK JP",
            "Noto Sans Mono CJK SC",
            "Noto Sans CJK JP",
            "Source Han Sans",
            // === Caucasian Scripts ===
            "Noto Sans Georgian", // Georgian ონი, etc.
            "DejaVu Sans",        // Has Georgian
            "Noto Sans Armenian",
            // === Middle East & Africa ===
            "Noto Sans Arabic",
            "Noto Sans Hebrew",
            "Noto Sans Ethiopic",
            "Noto Sans Thai",
            // === Indic ===
            "Noto Sans Devanagari",
            "Noto Sans Tamil",
            "Noto Sans Bengali",
            // === Symbols & Math ===
            "Noto Sans Symbols",
            "Noto Sans Symbols 2",
            "Noto Sans Math",
            "Symbola",
            // === Historic/Runic ===
            "Noto Sans Runic",
            "Junicode",
            "Segoe UI Historic",
            // === Emoji ===
            "Noto Emoji",
            "Noto Color Emoji",
        ]);

        names
    }

    /// Load a single fallback font by name
    fn load_fallback_font(name: &str) -> Option<FontVec> {
        use font_kit::family_name::FamilyName;
        use font_kit::properties::{Properties, Weight};
        use font_kit::source::SystemSource;

        let source = SystemSource::new();
        let handle = source
            .select_best_match(
                &[FamilyName::Title(name.to_string())],
                &Properties::new().weight(Weight::NORMAL),
            )
            .ok()?;
        let font = handle.load().ok()?;
        let data = font.copy_font_data()?;
        FontVec::try_from_vec(data.to_vec()).ok()
    }

    /// Search ALL system fonts for one that contains a specific character
    /// This is used as a last resort when our fallback list doesn't have the glyph
    fn find_font_for_char(ch: char) -> Option<FontVec> {
        use font_kit::source::SystemSource;

        let source = SystemSource::new();
        let handles = source.all_fonts().ok()?;

        for handle in handles {
            if let Ok(font) = handle.load() {
                // Check if this font has the character
                if font.glyph_for_char(ch).is_some() {
                    if let Some(data) = font.copy_font_data() {
                        if let Ok(font_vec) = FontVec::try_from_vec(data.to_vec()) {
                            #[cfg(debug_assertions)]
                            eprintln!(
                                "GUI: Found font for '{}' via system search: {:?}",
                                ch,
                                font.full_name()
                            );
                            return Some(font_vec);
                        }
                    }
                }
            }
        }
        None
    }
    /// Compute grid dimensions in cells
    fn compute_grid_dimensions(width: f32, height: f32, cell_w: f32, cell_h: f32) -> (u32, u32) {
        let usable_width = (width - MIN_PADDING * 2.0).max(0.0);
        let usable_height = (height - MIN_PADDING * 2.0).max(0.0);
        let cols = (usable_width / cell_w).floor() as u32;
        let rows = (usable_height / cell_h).floor() as u32;
        (cols.max(1), rows.max(1))
    }

    /// Compute centered grid offset (floor to pixel boundary for crisp rendering)
    fn compute_centered_offset(
        width: f32,
        height: f32,
        cols: u32,
        rows: u32,
        cell_w: f32,
        cell_h: f32,
    ) -> [f32; 2] {
        let grid_width = cols as f32 * cell_w;
        let grid_height = rows as f32 * cell_h;
        let offset_x = ((width - grid_width) / 2.0).floor().max(MIN_PADDING);
        let offset_y = ((height - grid_height) / 2.0).floor().max(MIN_PADDING);
        [offset_x, offset_y]
    }

    /// Resize the renderer
    pub fn resize(&mut self, new_size: (u32, u32)) {
        if new_size.0 > 0 && new_size.1 > 0 {
            self.size = new_size;
            self.config.width = new_size.0;
            self.config.height = new_size.1;
            self.surface.configure(&self.device, &self.config);

            // Compute centered offset
            let (cols, rows) = Self::compute_grid_dimensions(
                new_size.0 as f32,
                new_size.1 as f32,
                self.cell_width,
                self.cell_height,
            );
            let offset = Self::compute_centered_offset(
                new_size.0 as f32,
                new_size.1 as f32,
                cols,
                rows,
                self.cell_width,
                self.cell_height,
            );

            let uniforms = GridUniforms {
                screen_size: [new_size.0 as f32, new_size.1 as f32],
                cell_size: [self.cell_width, self.cell_height],
                grid_offset: offset,
                _padding: [0.0, 0.0],
            };
            self.queue
                .write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));
        }
    }

    /// Get grid dimensions in cells (accounting for padding)
    pub fn grid_size(&self) -> (u32, u32) {
        let metrics = GridMetrics::new(
            self.cell_width,
            self.cell_height,
            self.size.0 as f32,
            self.size.1 as f32,
        );
        metrics.grid_dimensions()
    }

    /// Draw a box drawing character procedurally for perfect alignment
    /// Box drawing chars (U+2500-U+257F) are rendered as lines that span edge-to-edge
    fn draw_box_char(bitmap: &mut [u8], ch: char, cell_w: u32, cell_h: u32) {
        let cx = cell_w / 2; // Center X
        let cy = cell_h / 2; // Center Y
        let light = 1u32.max(cell_h / 12); // Light line thickness
        let heavy = (light * 2).max(2); // Heavy line thickness

        // Parse character into line segments: (left, right, up, down)
        // Each value: 0=none, 1=light, 2=heavy
        // Based on Unicode Box Drawing character names and positions
        let (left, right, up, down) = match ch {
            // Light/heavy horizontal and vertical lines
            '─' => (1, 1, 0, 0), // U+2500
            '━' => (2, 2, 0, 0), // U+2501
            '│' => (0, 0, 1, 1), // U+2502
            '┃' => (0, 0, 2, 2), // U+2503
            // Down and Right corners (┌ variants)
            '┌' => (0, 1, 0, 1), // U+250C
            '┍' => (0, 2, 0, 1), // U+250D
            '┎' => (0, 1, 0, 2), // U+250E
            '┏' => (0, 2, 0, 2), // U+250F
            // Down and Left corners (┐ variants)
            '┐' => (1, 0, 0, 1), // U+2510
            '┑' => (2, 0, 0, 1), // U+2511
            '┒' => (1, 0, 0, 2), // U+2512
            '┓' => (2, 0, 0, 2), // U+2513
            // Up and Right corners (└ variants)
            '└' => (0, 1, 1, 0), // U+2514
            '┕' => (0, 2, 1, 0), // U+2515
            '┖' => (0, 1, 2, 0), // U+2516
            '┗' => (0, 2, 2, 0), // U+2517
            // Up and Left corners (┘ variants)
            '┘' => (1, 0, 1, 0), // U+2518
            '┙' => (2, 0, 1, 0), // U+2519
            '┚' => (1, 0, 2, 0), // U+251A
            '┛' => (2, 0, 2, 0), // U+251B
            // Vertical and Right tee (├ variants)
            '├' => (0, 1, 1, 1), // U+251C
            '┝' => (0, 2, 1, 1), // U+251D
            '┞' => (0, 1, 2, 1), // U+251E
            '┟' => (0, 1, 1, 2), // U+251F
            '┠' => (0, 1, 2, 2), // U+2520
            '┡' => (0, 2, 2, 1), // U+2521
            '┢' => (0, 2, 1, 2), // U+2522
            '┣' => (0, 2, 2, 2), // U+2523
            // Vertical and Left tee (┤ variants)
            '┤' => (1, 0, 1, 1), // U+2524
            '┥' => (2, 0, 1, 1), // U+2525
            '┦' => (1, 0, 2, 1), // U+2526
            '┧' => (1, 0, 1, 2), // U+2527
            '┨' => (1, 0, 2, 2), // U+2528
            '┩' => (2, 0, 2, 1), // U+2529
            '┪' => (2, 0, 1, 2), // U+252A
            '┫' => (2, 0, 2, 2), // U+252B
            // Down and Horizontal tee (┬ variants)
            '┬' => (1, 1, 0, 1), // U+252C
            '┭' => (2, 1, 0, 1), // U+252D
            '┮' => (1, 2, 0, 1), // U+252E
            '┯' => (2, 2, 0, 1), // U+252F
            '┰' => (1, 1, 0, 2), // U+2530
            '┱' => (2, 1, 0, 2), // U+2531
            '┲' => (1, 2, 0, 2), // U+2532
            '┳' => (2, 2, 0, 2), // U+2533
            // Up and Horizontal tee (┴ variants)
            '┴' => (1, 1, 1, 0), // U+2534
            '┵' => (2, 1, 1, 0), // U+2535
            '┶' => (1, 2, 1, 0), // U+2536
            '┷' => (2, 2, 1, 0), // U+2537
            '┸' => (1, 1, 2, 0), // U+2538
            '┹' => (2, 1, 2, 0), // U+2539
            '┺' => (1, 2, 2, 0), // U+253A
            '┻' => (2, 2, 2, 0), // U+253B
            // Cross (┼ variants)
            '┼' => (1, 1, 1, 1), // U+253C
            '┽' => (2, 1, 1, 1), // U+253D
            '┾' => (1, 2, 1, 1), // U+253E
            '┿' => (2, 2, 1, 1), // U+253F
            '╀' => (1, 1, 2, 1), // U+2540
            '╁' => (1, 1, 1, 2), // U+2541
            '╂' => (1, 1, 2, 2), // U+2542
            '╃' => (2, 1, 2, 1), // U+2543
            '╄' => (1, 2, 2, 1), // U+2544
            '╅' => (2, 1, 1, 2), // U+2545
            '╆' => (1, 2, 1, 2), // U+2546
            '╇' => (2, 2, 2, 1), // U+2547
            '╈' => (2, 2, 1, 2), // U+2548
            '╉' => (2, 1, 2, 2), // U+2549
            '╊' => (1, 2, 2, 2), // U+254A
            '╋' => (2, 2, 2, 2), // U+254B
            // Double line characters
            '═' => (2, 2, 0, 0), // U+2550 double horizontal
            '║' => (0, 0, 2, 2), // U+2551 double vertical
            '╔' => (0, 2, 0, 2), // U+2554 double down-right
            '╗' => (2, 0, 0, 2), // U+2557 double down-left
            '╚' => (0, 2, 2, 0), // U+255A double up-right
            '╝' => (2, 0, 2, 0), // U+255D double up-left
            '╠' => (0, 2, 2, 2), // U+2560 double vert-right
            '╣' => (2, 0, 2, 2), // U+2563 double vert-left
            '╦' => (2, 2, 0, 2), // U+2566 double down-horiz
            '╩' => (2, 2, 2, 0), // U+2569 double up-horiz
            '╬' => (2, 2, 2, 2), // U+256C double cross
            // Default fallback for other box chars
            _ => {
                let code = ch as u32;
                if (0x2500..=0x257F).contains(&code) {
                    (1, 1, 1, 1) // Default to light cross
                } else {
                    return; // Not a box drawing char
                }
            }
        };

        // Draw horizontal line segments
        if left > 0 || right > 0 {
            let t = if left == 2 || right == 2 {
                heavy
            } else {
                light
            };
            let start_x = if left > 0 { 0 } else { cx };
            let end_x = if right > 0 { cell_w } else { cx + t };
            let y_start = cy.saturating_sub(t / 2);
            let y_end = (cy + (t + 1) / 2).min(cell_h);
            for y in y_start..y_end {
                for x in start_x..end_x.min(cell_w) {
                    let idx = (y * cell_w + x) as usize;
                    if idx < bitmap.len() {
                        bitmap[idx] = 255;
                    }
                }
            }
        }

        // Draw vertical line segments
        if up > 0 || down > 0 {
            let t = if up == 2 || down == 2 { heavy } else { light };
            let start_y = if up > 0 { 0 } else { cy };
            let end_y = if down > 0 { cell_h } else { cy + t };
            let x_start = cx.saturating_sub(t / 2);
            let x_end = (cx + (t + 1) / 2).min(cell_w);
            for y in start_y..end_y.min(cell_h) {
                for x in x_start..x_end {
                    let idx = (y * cell_w + x) as usize;
                    if idx < bitmap.len() {
                        bitmap[idx] = 255;
                    }
                }
            }
        }
    }

    /// Draw a block element character procedurally
    /// Block elements (U+2580-U+259F) are rendered by filling portions of the cell
    fn draw_block_char(bitmap: &mut [u8], ch: char, cell_w: u32, cell_h: u32) {
        let code = ch as u32 - 0x2580;

        // Determine fill pattern based on character
        let (x_start, x_end, y_start, y_end, intensity) = match code {
            0x00 => (0, cell_w, 0, cell_h / 2, 255u8), // ▀ upper half
            0x04 => (0, cell_w, cell_h / 2, cell_h, 255), // ▄ lower half
            0x08 => (0, cell_w, 0, cell_h, 255),       // █ full block
            0x0C => (cell_w / 2, cell_w, 0, cell_h, 255), // ▐ right half
            0x10 => (0, cell_w, 0, cell_h, 64),        // ░ light shade
            0x11 => (0, cell_w, 0, cell_h, 128),       // ▒ medium shade
            0x12 => (0, cell_w, 0, cell_h, 192),       // ▓ dark shade
            0x0F => (0, cell_w / 2, 0, cell_h, 255),   // ▌ left half
            _ => (0, cell_w, 0, cell_h, 255),          // default: full
        };

        for y in y_start..y_end.min(cell_h) {
            for x in x_start..x_end.min(cell_w) {
                let idx = (y * cell_w + x) as usize;
                if idx < bitmap.len() {
                    bitmap[idx] = intensity;
                }
            }
        }
    }

    /// Ensure a glyph is in the atlas, returns UV coordinates
    /// Glyphs are rasterized into cell-sized regions for 1:1 pixel mapping
    fn ensure_glyph(&mut self, ch: char) -> GlyphInfo {
        if let Some(info) = self.glyph_cache.get(&ch) {
            return *info;
        }

        // Cell dimensions (integer for atlas storage)
        // Use round() to match the calculation in new()
        let cell_w = self.cell_width.round() as u32;
        let cell_h = self.cell_height.round() as u32;

        // Check if we need to move to next row
        if self.atlas_cursor_x + cell_w > self.atlas_width {
            self.atlas_cursor_x = 0;
            self.atlas_cursor_y += cell_h + 1;
        }

        // Check atlas capacity - reset if 80% full to prevent degradation
        // This is a simple LRU strategy: when we run low on space, start fresh
        let atlas_usage = (self.atlas_cursor_y as f64 / self.atlas_height as f64) * 100.0;
        if atlas_usage > 80.0 || self.atlas_cursor_y + cell_h > self.atlas_height {
            #[cfg(debug_assertions)]
            eprintln!(
                "GUI: Atlas reset triggered at {:.1}% usage ({} cached glyphs)",
                atlas_usage,
                self.glyph_cache.len()
            );

            // Reset atlas state
            self.glyph_cache.clear();
            self.atlas_cursor_x = 0;
            self.atlas_cursor_y = 0;
        }

        // Create cell-sized bitmap
        let mut bitmap = vec![0u8; (cell_w * cell_h) as usize];

        // Check if this is a box drawing character - render procedurally for perfect alignment
        let is_box_drawing = ('\u{2500}'..='\u{257F}').contains(&ch);
        let is_block_element = ('\u{2580}'..='\u{259F}').contains(&ch);

        if is_box_drawing {
            // Render box drawing characters procedurally
            Self::draw_box_char(&mut bitmap, ch, cell_w, cell_h);
        } else if is_block_element {
            // Render block elements procedurally
            Self::draw_block_char(&mut bitmap, ch, cell_w, cell_h);
        } else {
            // Regular font-based rendering for non-box-drawing characters

            // Helper closure to rasterize a glyph from a given font
            // Returns true if glyph was successfully rasterized
            let try_rasterize = |font: &FontVec,
                                 scale: PxScale,
                                 ch: char,
                                 bitmap: &mut [u8],
                                 cell_w: u32,
                                 cell_h: u32|
             -> bool {
                use ab_glyph::Font;

                // First check if font has this glyph at all (glyph_id 0 = .notdef/missing)
                let glyph_id = font.glyph_id(ch);
                if glyph_id.0 == 0 {
                    return false; // Font doesn't have this character
                }

                let scaled_font = font.as_scaled(scale);
                let glyph = scaled_font.scaled_glyph(ch);

                if let Some(outlined) = scaled_font.outline_glyph(glyph) {
                    let bounds = outlined.px_bounds();
                    let glyph_w = bounds.width() as u32;

                    // Horizontal positioning - center in cell
                    let offset_x = ((cell_w.saturating_sub(glyph_w)) / 2) as i32;

                    // Vertical positioning - align baseline
                    let descent = scaled_font.descent();
                    let baseline_from_bottom = (-descent).ceil() as i32 + 1;
                    let baseline_y = cell_h as i32 - baseline_from_bottom;
                    let offset_y = baseline_y + bounds.min.y.round() as i32;

                    // Draw glyph with offset
                    outlined.draw(|x, y, c| {
                        let px = x as i32 + offset_x;
                        let py = y as i32 + offset_y;
                        if px >= 0 && py >= 0 && (px as u32) < cell_w && (py as u32) < cell_h {
                            let idx = (py as u32 * cell_w + px as u32) as usize;
                            if idx < bitmap.len() {
                                bitmap[idx] = (c * 255.0) as u8;
                            }
                        }
                    });
                    return true;
                }

                // Font has glyph ID but no outline
                false
            };

            // Try primary font first
            let mut rasterized =
                try_rasterize(&self.font, self.font_scale, ch, &mut bitmap, cell_w, cell_h);

            // If primary font doesn't have this glyph, try already-loaded fallbacks
            if !rasterized {
                for fallback in &self.loaded_fallbacks {
                    if try_rasterize(fallback, self.font_scale, ch, &mut bitmap, cell_w, cell_h) {
                        rasterized = true;
                        break;
                    }
                }
            }

            // If still not found, progressively load new fallback fonts
            if !rasterized {
                while self.next_fallback_index < self.fallback_font_names.len() {
                    let name = self.fallback_font_names[self.next_fallback_index];
                    self.next_fallback_index += 1;

                    if let Some(font_vec) = Self::load_fallback_font(name) {
                        #[cfg(debug_assertions)]
                        eprintln!("GUI: Lazy-loaded fallback font: {}", name);

                        // Check if this font has the glyph
                        use ab_glyph::Font;
                        let has_glyph = font_vec.glyph_id(ch).0 != 0;

                        // Always keep the loaded font for future use
                        self.loaded_fallbacks.push(font_vec);

                        if has_glyph {
                            // Try to rasterize from the newly loaded font
                            if let Some(new_font) = self.loaded_fallbacks.last() {
                                if try_rasterize(
                                    new_font,
                                    self.font_scale,
                                    ch,
                                    &mut bitmap,
                                    cell_w,
                                    cell_h,
                                ) {
                                    rasterized = true;
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            // Last resort: search ALL system fonts for this character
            if !rasterized && ch != ' ' {
                if let Some(font_vec) = Self::find_font_for_char(ch) {
                    // Check and try to rasterize
                    use ab_glyph::Font;
                    if font_vec.glyph_id(ch).0 != 0 {
                        if try_rasterize(
                            &font_vec,
                            self.font_scale,
                            ch,
                            &mut bitmap,
                            cell_w,
                            cell_h,
                        ) {
                            rasterized = true;
                        }
                    }
                    // Keep this font for future characters
                    self.loaded_fallbacks.push(font_vec);
                }
            }

            // If still not rasterized and not a space, draw a placeholder box
            if !rasterized && ch != ' ' {
                #[cfg(debug_assertions)]
                eprintln!("GUI: No glyph found for '{}'", ch);

                // Draw a simple box outline as "missing glyph" indicator
                let margin = 2u32;
                for x in margin..(cell_w - margin) {
                    if (margin as usize) < bitmap.len() / cell_w as usize {
                        bitmap[(margin * cell_w + x) as usize] = 128; // Top
                    }
                    if ((cell_h - margin - 1) as usize) < bitmap.len() / cell_w as usize {
                        bitmap[((cell_h - margin - 1) * cell_w + x) as usize] = 128; // Bottom
                    }
                }
                for y in margin..(cell_h - margin) {
                    bitmap[(y * cell_w + margin) as usize] = 128; // Left
                    bitmap[(y * cell_w + cell_w - margin - 1) as usize] = 128; // Right
                }
            }
        }

        // Upload cell-sized region to atlas
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.glyph_atlas,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: self.atlas_cursor_x,
                    y: self.atlas_cursor_y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &bitmap,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(cell_w),
                rows_per_image: Some(cell_h),
            },
            wgpu::Extent3d {
                width: cell_w,
                height: cell_h,
                depth_or_array_layers: 1,
            },
        );

        let info = GlyphInfo {
            uv_x: self.atlas_cursor_x as f32 / self.atlas_width as f32,
            uv_y: self.atlas_cursor_y as f32 / self.atlas_height as f32,
            uv_w: cell_w as f32 / self.atlas_width as f32,
            uv_h: cell_h as f32 / self.atlas_height as f32,
            offset_x: 0.0,
            offset_y: 0.0,
            width: cell_w as f32,
            height: cell_h as f32,
        };

        // Advance cursor
        self.atlas_cursor_x += cell_w + 1;

        self.glyph_cache.insert(ch, info);
        info
    }

    /// Render the screen buffer
    pub fn render(&mut self, buffer: &ScreenBuffer) -> Result<(), wgpu::SurfaceError> {
        // Build cell instances
        let width = buffer.width as usize;
        let height = buffer.height as usize;
        let mut instances: Vec<CellVertex> = Vec::with_capacity(width * height);

        for y in 0..height {
            for x in 0..width {
                let idx = y * width + x;
                let cell = &buffer.cells[idx];

                let glyph_info = if cell.ch != ' ' && cell.ch != '\0' {
                    self.ensure_glyph(cell.ch)
                } else {
                    GlyphInfo {
                        uv_x: 0.0,
                        uv_y: 0.0,
                        uv_w: 0.0,
                        uv_h: 0.0,
                        offset_x: 0.0,
                        offset_y: 0.0,
                        width: 0.0,
                        height: 0.0,
                    }
                };

                instances.push(CellVertex {
                    cell_pos: [x as f32, y as f32],
                    fg_color: cell.fg.to_rgba_f32(),
                    bg_color: cell.bg.to_rgba_f32(),
                    glyph_uv: [
                        glyph_info.uv_x,
                        glyph_info.uv_y,
                        glyph_info.uv_w,
                        glyph_info.uv_h,
                    ],
                    glyph_metrics: [
                        glyph_info.offset_x,
                        glyph_info.offset_y,
                        glyph_info.width,
                        glyph_info.height,
                    ],
                });
            }
        }

        let instance_count = instances.len();
        if instance_count == 0 {
            return Ok(());
        }

        #[cfg(debug_assertions)]
        {
            use std::sync::atomic::{AtomicU32, Ordering};
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let count = COUNTER.fetch_add(1, Ordering::Relaxed);
            if count % 60 == 0 {
                eprintln!(
                    "GUI: rendering {} instances, atlas_y={}",
                    instance_count, self.atlas_cursor_y
                );
            }
        }

        if instance_count > self.instance_capacity {
            self.instance_capacity = instance_count.max(1024);
            self.instance_buffer = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Cell Instance Buffer"),
                size: (self.instance_capacity * std::mem::size_of::<CellVertex>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }

        // Upload instance data
        if let Some(ref buffer) = self.instance_buffer {
            self.queue
                .write_buffer(buffer, 0, bytemuck::cast_slice(&instances));
        }

        // Create bind group
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Grid Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.glyph_atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.glyph_sampler),
                },
            ],
        });

        // Get surface texture
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Create command encoder
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        // Render pass
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Grid Render Pass"),
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
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            render_pass.set_pipeline(&self.grid_pipeline);
            render_pass.set_bind_group(0, &bind_group, &[]);

            if let Some(ref instance_buffer) = self.instance_buffer {
                render_pass.set_vertex_buffer(0, instance_buffer.slice(..));
                render_pass.draw(0..6, 0..instance_count as u32);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }

    /// Get current size
    pub fn size(&self) -> (u32, u32) {
        self.size
    }
}

impl Copy for GlyphInfo {}
impl Clone for GlyphInfo {
    fn clone(&self) -> Self {
        *self
    }
}

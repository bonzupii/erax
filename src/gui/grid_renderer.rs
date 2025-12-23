//! Complete GPU-accelerated grid renderer for era GUI mode
//!
//! This is a terminal emulator that renders the TUI's ScreenBuffer using wgpu.
//! Each cell is rendered as a colored quad with a glyph texture.

use bytemuck::{Pod, Zeroable};
use fontdue::Font;
use pollster::block_on;
use std::collections::HashMap;
use wgpu::util::DeviceExt;

use crate::core::geometry::GridMetrics;
use crate::gui::font_loader::FontLoader;
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

    // Font loading and fallback management
    font_loader: FontLoader,
    font_size: f32,
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

        // Load font via FontLoader
        let font_loader = FontLoader::new(font_path)?;

        // Calculate cell dimensions using fontdue metrics
        // fontdue returns metrics at the specified pixel size
        let (m_metrics, _) = font_loader.primary.rasterize('M', scaled_font_size);
        let cell_width = m_metrics.advance_width.round();

        // Get line height from font metrics
        let line_metrics = font_loader
            .primary
            .horizontal_line_metrics(scaled_font_size);
        let cell_height = match line_metrics {
            Some(lm) => (lm.ascent - lm.descent + lm.line_gap).round(),
            None => scaled_font_size.round(), // Fallback
        };

        #[cfg(debug_assertions)]
        {
            eprintln!(
                "GUI: cell={}x{}, font_size={}",
                cell_width, cell_height, scaled_font_size
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
            font_loader,
            font_size: scaled_font_size,
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

        // Helper to rasterize a glyph from a fontdue Font
        // Returns true if glyph was successfully rasterized
        let try_rasterize_fontdue = |font: &Font,
                                     font_size: f32,
                                     ch: char,
                                     bitmap: &mut [u8],
                                     cell_w: u32,
                                     cell_h: u32|
         -> bool {
            // Check if font has this glyph (glyph_index 0 = .notdef/missing)
            if font.lookup_glyph_index(ch) == 0 {
                return false;
            }

            // Rasterize the glyph
            let (metrics, glyph_bitmap) = font.rasterize(ch, font_size);

            if glyph_bitmap.is_empty() {
                return false; // No bitmap (space or similar)
            }

            let glyph_w = metrics.width as u32;
            let glyph_h = metrics.height as u32;

            // Horizontal positioning - center in cell
            let offset_x = ((cell_w.saturating_sub(glyph_w)) / 2) as i32;

            // Vertical positioning - fontdue provides ymin (negative for below baseline)
            // We need to position relative to cell baseline
            let line_metrics = font.horizontal_line_metrics(font_size);
            let descent = line_metrics.map(|lm| lm.descent).unwrap_or(0.0);
            let baseline_from_bottom = (-descent).ceil() as i32 + 1;
            let baseline_y = cell_h as i32 - baseline_from_bottom;
            let offset_y = baseline_y - metrics.height as i32 - metrics.ymin;

            // Copy glyph bitmap into cell bitmap
            for gy in 0..glyph_h {
                for gx in 0..glyph_w {
                    let px = gx as i32 + offset_x;
                    let py = gy as i32 + offset_y;
                    if px >= 0 && py >= 0 && (px as u32) < cell_w && (py as u32) < cell_h {
                        let src_idx = (gy * glyph_w + gx) as usize;
                        let dst_idx = (py as u32 * cell_w + px as u32) as usize;
                        if src_idx < glyph_bitmap.len() && dst_idx < bitmap.len() {
                            bitmap[dst_idx] = glyph_bitmap[src_idx];
                        }
                    }
                }
            }
            true
        };

        // Track if we successfully rasterized this glyph
        let mut rasterized = false;

        // Check negative cache first - skip if we know this char has no font
        if self.font_loader.is_known_missing(ch) {
            // Skip to placeholder rendering (rasterized stays false)
        } else {
            // Try primary font first
            rasterized = try_rasterize_fontdue(
                &self.font_loader.primary,
                self.font_size,
                ch,
                &mut bitmap,
                cell_w,
                cell_h,
            );

            // If primary font doesn't have this glyph, try already-loaded fallbacks
            if !rasterized {
                for fallback in &self.font_loader.fallbacks {
                    if try_rasterize_fontdue(
                        fallback,
                        self.font_size,
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

            // If still not found, progressively load new fallback fonts
            if !rasterized {
                while self.font_loader.has_more_fallbacks() {
                    if self.font_loader.load_next_fallback() {
                        // Try the newly loaded font
                        if let Some(new_font) = self.font_loader.fallbacks.last() {
                            if try_rasterize_fontdue(
                                new_font,
                                self.font_size,
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

            // If still not found after all fallbacks, mark as missing
            if !rasterized && ch != ' ' {
                self.font_loader.mark_missing(ch);
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

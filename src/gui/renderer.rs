//! GPU Renderer - Fixed version
//!
//! Fixes:
//! 1. Don't clear char_cache on atlas reset (char->glyph mapping is stable)
//! 2. Use integer cell dimensions to prevent rounding drift
//! 3. Proper baseline from font metrics

use bytemuck::{Pod, Zeroable};
use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, SwashContent};
use pollster::block_on;
use std::collections::HashMap;
use std::sync::Arc;
use winit::window::Window;

use crate::core::geometry::GridMetrics;
use crate::terminal::display::ScreenBuffer;

use super::atlas::{Atlas, GlyphKey};
use super::font_manager::FontManager;
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

/// Cached glyph info - independent of atlas layout
#[derive(Clone, Copy)]
struct CharGlyph {
    key: GlyphKey,
    cache_key: cosmic_text::CacheKey,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    size: (u32, u32),
    font_manager: FontManager,
    atlas: Atlas,
    quad_renderer: QuadRenderer,

    // Char -> glyph mapping (NEVER cleared - stable across atlas resets)
    char_cache: HashMap<char, CharGlyph>,
    shape_buffer: Buffer,

    last_atlas_gen: u64,

    glyph_pipeline: wgpu::RenderPipeline,
    glyph_bind_group_layout: wgpu::BindGroupLayout,
    glyph_uniform_buffer: wgpu::Buffer,
    glyph_bind_group: Option<wgpu::BindGroup>,
    glyph_buffer: Option<wgpu::Buffer>,
    glyph_capacity: usize,

    instances: Vec<GlyphInstance>,

    pub metrics: GridMetrics,
}

impl Renderer {
    pub fn new(
        window: Arc<Window>,
        font_path: Option<&str>,
        font_size: f32,
        scale_factor: f64,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let size = window.inner_size();
        let scaled_font_size = font_size * scale_factor as f32;
        // Use integer line height to prevent rounding drift
        let line_height = (scaled_font_size * 1.2).round();

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        let surface = instance.create_surface(window)?;
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .map_err(|e| format!("No GPU: {}", e))?;

        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("erax"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: wgpu::MemoryHints::Performance,
            ..Default::default()
        }))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .copied()
            .unwrap_or(wgpu::TextureFormat::Bgra8Unorm);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let mut font_manager = FontManager::new(scaled_font_size, line_height, font_path);
        let (cw, ch) = font_manager.cell_dimensions();

        // Round cell dimensions to integers to prevent gaps
        let cell_width = cw.round();
        let cell_height = ch.ceil();
        let metrics = GridMetrics::new(
            cell_width,
            cell_height,
            size.width as f32,
            size.height as f32,
        );

        let atlas = Atlas::new(&device, 2048, 2048);
        let quad_renderer = QuadRenderer::new(&device, format, 8192);

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
            bind_group_layouts: &[&bind_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Glyph Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
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
                }],
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
            multiview: None,
            cache: None,
        });

        let shape_buffer = Buffer::new(
            &mut font_manager.font_system,
            Metrics::new(scaled_font_size, line_height),
        );

        Ok(Self {
            device,
            queue,
            surface,
            config,
            size: (size.width, size.height),
            font_manager,
            atlas,
            quad_renderer,
            char_cache: HashMap::new(),
            shape_buffer,
            last_atlas_gen: 0,
            glyph_pipeline: pipeline,
            glyph_bind_group_layout: bind_layout,
            glyph_uniform_buffer: uniform_buf,
            glyph_bind_group: None,
            glyph_buffer: None,
            glyph_capacity: 0,
            instances: Vec::new(),
            metrics,
        })
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
    pub fn preload_fonts_for_buffer(&mut self, _: &ScreenBuffer) {}

    /// Get glyph info for a character (cached forever - independent of atlas)
    fn get_char_glyph(&mut self, ch: char) -> Option<CharGlyph> {
        if let Some(cg) = self.char_cache.get(&ch) {
            return Some(*cg);
        }

        let mut attrs = Attrs::new();
        if let Some(ref fam) = self.font_manager.primary_family {
            attrs = attrs.family(Family::Name(fam));
        } else {
            attrs = attrs.family(Family::Monospace);
        }

        let s = ch.to_string();
        self.shape_buffer.set_text(
            &mut self.font_manager.font_system,
            &s,
            &attrs,
            Shaping::Advanced,
            None,
        );
        self.shape_buffer
            .shape_until_scroll(&mut self.font_manager.font_system, false);

        for run in self.shape_buffer.layout_runs() {
            if let Some(g) = run.glyphs.first() {
                let pg = g.physical((0.0, 0.0), 1.0);
                let cg = CharGlyph {
                    key: GlyphKey {
                        font_id: pg.cache_key.font_id,
                        glyph_index: pg.cache_key.glyph_id as u32,
                    },
                    cache_key: pg.cache_key,
                };
                self.char_cache.insert(ch, cg);
                return Some(cg);
            }
        }
        None
    }

    pub fn render(&mut self, screen_buffer: &ScreenBuffer) -> Result<(), wgpu::SurfaceError> {
        let cols = screen_buffer.width as usize;
        let rows = screen_buffer.height as usize;

        // Reset atlas if full - but DON'T clear char_cache (it's stable!)
        if self.atlas.is_full() {
            self.atlas.reset();
        }
        let atlas_changed = self.atlas.generation() != self.last_atlas_gen;

        // Backgrounds
        let bg: Vec<u32> = screen_buffer
            .cells
            .iter()
            .map(|c| c.bg.to_packed_rgba())
            .collect();

        // Glyphs
        self.instances.clear();

        // Use integer math to prevent rounding drift
        let cw = self.metrics.cell_width;
        let ch = self.metrics.cell_height;
        let ox = self.metrics.offset_x;
        let oy = self.metrics.offset_y;
        let ascent = self.font_manager.ascent;

        for row in 0..rows {
            // Integer row * integer cell_height = no drift
            let y = oy + (row as f32) * ch;

            for col in 0..cols {
                let idx = row * cols + col;
                let cell = &screen_buffer.cells[idx];

                if cell.hidden || cell.ch == ' ' || cell.ch == '\0' {
                    continue;
                }

                let x = ox + (col as f32) * cw;
                let color = cell.fg.to_rgba_f32();

                if let Some(char_glyph) = self.get_char_glyph(cell.ch) {
                    // Check atlas first, then rasterize if needed
                    let cached = if let Some(g) = self.atlas.get(char_glyph.key) {
                        Some(g)
                    } else if let Some(img) = self
                        .font_manager
                        .swash_cache
                        .get_image(&mut self.font_manager.font_system, char_glyph.cache_key)
                    {
                        if img.placement.width > 0 && img.placement.height > 0 {
                            if let Some(slot) = self
                                .atlas
                                .allocate(img.placement.width, img.placement.height)
                            {
                                let px = (img.placement.width * img.placement.height) as usize;
                                let data: Vec<u8> = match img.content {
                                    SwashContent::Mask => img.data.clone(),
                                    SwashContent::Color => {
                                        img.data.chunks(4).map(|c| c[3]).collect()
                                    }
                                    _ => {
                                        if img.data.len() == px * 3 {
                                            img.data
                                                .chunks(3)
                                                .map(|c| {
                                                    ((c[0] as u32 + c[1] as u32 + c[2] as u32) / 3)
                                                        as u8
                                                })
                                                .collect()
                                        } else {
                                            img.data.clone()
                                        }
                                    }
                                };
                                self.atlas.insert(
                                    char_glyph.key,
                                    slot,
                                    data,
                                    img.placement.left as f32,
                                    img.placement.top as f32,
                                );
                                self.atlas.get(char_glyph.key)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    if let Some(g) = cached {
                        // Position: cell origin + bearing offsets
                        let gx = x + g.bearing_x;
                        let gy = y + ascent - g.bearing_y;

                        self.instances.push(GlyphInstance {
                            pos: [gx, gy],
                            size: [g.width, g.height],
                            uv_pos: [g.uv_x, g.uv_y],
                            uv_size: [g.uv_w, g.uv_h],
                            color,
                        });
                    }
                }
            }
        }

        self.last_atlas_gen = self.atlas.generation();
        self.atlas.flush(&self.queue);

        // Update GPU
        let uniforms = BackgroundUniforms {
            screen_size: [self.size.0 as f32, self.size.1 as f32],
            cell_size: [cw, ch],
            grid_offset: [ox, oy],
            grid_dims: [cols as u32, rows as u32],
        };
        self.quad_renderer.update_uniforms(&self.queue, &uniforms);
        self.quad_renderer
            .upload_colors(&self.device, &self.queue, &bg);

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
        let out = self.surface.get_current_texture()?;
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

            if !self.instances.is_empty() {
                if let (Some(b), Some(bg)) = (&self.glyph_buffer, &self.glyph_bind_group) {
                    rp.set_pipeline(&self.glyph_pipeline);
                    rp.set_bind_group(0, bg, &[]);
                    rp.set_vertex_buffer(0, b.slice(..));
                    rp.draw(0..6, 0..self.instances.len() as u32);
                }
            }
        }

        self.queue.submit(std::iter::once(enc.finish()));
        out.present();
        Ok(())
    }
}

//! Chrome compositing pass: cell backgrounds plus GUI chrome.
//!
//! Uploads the flat color array as an RGBA8 texture (one texel per cell)
//! and the cell roles as R32Uint; the composite shader indexes both by
//! fragment position and renders the chrome treatment (glass, specular,
//! shadow, or the Solid legacy panels). The color texture is filterable
//! so the glass blur kernel samples smoothly; the role texture stays
//! integer-exact. Filterable float textures work on WebGL2, where
//! fragment shaders cannot bind storage buffers.

use bytemuck::{Pod, Zeroable};

/// Uniforms for the chrome compositing pass.
///
/// The struct layout must match `shaders/composite.wgsl`'s `Uniforms`:
/// three `vec2<f32>`, one `vec2<u32>`, one `vec4<f32>`, then eight `f32`
/// chrome knobs (80 bytes total).
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct CompositeUniforms {
    pub screen_size: [f32; 2],
    pub cell_size: [f32; 2],
    pub grid_offset: [f32; 2],
    pub grid_dims: [u32; 2],
    /// Theme background: paints out-of-grid fragments.
    pub bg: [f32; 4],
    /// Frosted panel backgrounds (0.0/1.0).
    pub glass: f32,
    /// Glass blur kernel radius in pixels (0.0 = tint only).
    pub blur_radius_px: f32,
    /// Glass tint: mix of the blurred fill toward the theme background.
    pub glass_tint: f32,
    /// Specular edge highlight strength (0.0 = off).
    pub specular: f32,
    /// Drop shadow strength (0.0 = off).
    pub shadow_strength: f32,
    /// Drop shadow reach in pixels (0.0 = off).
    pub shadow_px: f32,
    /// Solid-style panel luminance step, signed from the theme's
    /// darkness (0.0 = off).
    pub panel_shift: f32,
    /// Solid-style panel run edge step (0.0 = off).
    pub panel_edge: f32,
}

/// Knobs for [`resolve_chrome_uniforms`]: the renderer fills these from
/// `crate::config::chrome`, tests fill them with explicit values so no
/// test asserts a config constant.
#[derive(Clone, Copy, Debug)]
pub struct ChromeKnobs {
    /// Master switch; false zeroes every slot (the identity).
    pub enabled: bool,
    /// Frosted panel backgrounds on.
    pub glass: bool,
    /// Backdrop blur inside the glass treatment.
    pub blur: bool,
    /// Blur kernel radius in pixels.
    pub blur_radius_px: f32,
    /// Glass tint toward the theme background (0.0..=1.0).
    pub glass_tint: f32,
    /// Specular edge highlights on.
    pub specular: bool,
    /// Specular highlight strength.
    pub specular_strength: f32,
    /// Drop shadows under floating panels.
    pub shadow: bool,
    /// Shadow strength (0.0..=1.0).
    pub shadow_strength: f32,
    /// Shadow reach in pixels.
    pub shadow_px: f32,
    /// Legacy opaque panel lift + run edge (the Solid style).
    pub solid_panels: bool,
    /// Unsigned panel luminance magnitude.
    pub panel_shift: f32,
    /// Unsigned panel edge magnitude.
    pub panel_edge_delta: f32,
}

/// Resolve the composite-pass chrome uniforms from the knobs and the
/// theme's darkness.
///
/// Pure over its inputs: disabled effects contribute exactly 0.0, which
/// the shader treats as the identity, and the Solid panel steps are
/// signed here — added on dark themes, subtracted on light ones — so
/// the WGSL stays theme-agnostic. Returns `[glass, blur_radius_px,
/// glass_tint, specular, shadow_strength, shadow_px, panel_shift,
/// panel_edge]`.
pub fn resolve_chrome_uniforms(knobs: ChromeKnobs, is_dark: bool) -> [f32; 8] {
    if !knobs.enabled {
        return [0.0; 8];
    }
    let sign = if is_dark { 1.0 } else { -1.0 };
    [
        if knobs.glass { 1.0 } else { 0.0 },
        if knobs.glass && knobs.blur {
            knobs.blur_radius_px
        } else {
            0.0
        },
        if knobs.glass { knobs.glass_tint } else { 0.0 },
        if knobs.specular {
            knobs.specular_strength
        } else {
            0.0
        },
        if knobs.shadow {
            knobs.shadow_strength
        } else {
            0.0
        },
        if knobs.shadow { knobs.shadow_px } else { 0.0 },
        if knobs.solid_panels {
            sign * knobs.panel_shift
        } else {
            0.0
        },
        if knobs.solid_panels {
            sign * knobs.panel_edge_delta
        } else {
            0.0
        },
    ]
}

/// Manages the chrome compositing pass
pub struct QuadRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform_buffer: wgpu::Buffer,
    color_texture: wgpu::Texture,
    color_view: wgpu::TextureView,
    role_texture: wgpu::Texture,
    role_view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    bind_group: wgpu::BindGroup,
    tex_cols: u32,
    tex_rows: u32,
}

impl QuadRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Composite Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/composite.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Composite Bind Group Layout"),
            entries: &[
                // Uniforms
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
                // Cell colors as a filterable RGBA8 texture (one texel
                // per cell); the glass blur kernel samples it with
                // bilinear filtering.
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
                // Sampler for the blur kernel.
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // Cell semantic roles as a u32 texture (one texel per
                // cell, the `CellRole` discriminant in the low byte).
                // Integer-exact: roles never filter.
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Composite Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Composite Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
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
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Composite Uniforms"),
            size: std::mem::size_of::<CompositeUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Composite Blur Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        // 1x1 placeholders; sized to the grid on the first upload.
        let (color_texture, color_view) = Self::cell_texture(
            device,
            1,
            1,
            wgpu::TextureFormat::Rgba8Unorm,
            "Composite Colors",
        );
        let (role_texture, role_view) = Self::cell_texture(
            device,
            1,
            1,
            wgpu::TextureFormat::R32Uint,
            "Composite Roles",
        );

        let bind_group = Self::make_bind_group(
            device,
            &bind_group_layout,
            &uniform_buffer,
            &color_view,
            &sampler,
            &role_view,
        );

        Self {
            pipeline,
            bind_group_layout,
            uniform_buffer,
            color_texture,
            color_view,
            role_texture,
            role_view,
            sampler,
            bind_group,
            tex_cols: 1,
            tex_rows: 1,
        }
    }

    /// One cell-grid texture plus its view, in the given format.
    fn cell_texture(
        device: &wgpu::Device,
        cols: u32,
        rows: u32,
        format: wgpu::TextureFormat,
        label: &'static str,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: cols,
                height: rows,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    /// Bind group over the uniforms, the filterable color texture, its
    /// sampler, and the integer role texture.
    fn make_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        uniform_buffer: &wgpu::Buffer,
        color_view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        role_view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Composite Bind Group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(color_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(role_view),
                },
            ],
        })
    }

    /// Update uniforms
    pub fn update_uniforms(&self, queue: &wgpu::Queue, uniforms: &CompositeUniforms) {
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(uniforms));
    }

    /// Upload background colors and cell roles for a `cols` x `rows` grid.
    ///
    /// `colors` holds one packed color `u32` per cell (R,G,B,A byte
    /// order, as produced by `Color::to_packed_rgba`); `roles` one
    /// `CellRole` discriminant per cell. Both textures resize together
    /// when the grid dims change.
    pub fn upload_cells(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        colors: &[u32],
        roles: &[u32],
        cols: u32,
        rows: u32,
    ) {
        // Resize the textures when the grid dims change.
        if cols != self.tex_cols || rows != self.tex_rows {
            self.tex_cols = cols.max(1);
            self.tex_rows = rows.max(1);
            (self.color_texture, self.color_view) = Self::cell_texture(
                device,
                self.tex_cols,
                self.tex_rows,
                wgpu::TextureFormat::Rgba8Unorm,
                "Composite Colors",
            );
            (self.role_texture, self.role_view) = Self::cell_texture(
                device,
                self.tex_cols,
                self.tex_rows,
                wgpu::TextureFormat::R32Uint,
                "Composite Roles",
            );

            // Recreate bind group
            self.bind_group = Self::make_bind_group(
                device,
                &self.bind_group_layout,
                &self.uniform_buffer,
                &self.color_view,
                &self.sampler,
                &self.role_view,
            );
        }

        let expected = (self.tex_cols * self.tex_rows) as usize;
        debug_assert_eq!(colors.len(), expected);
        debug_assert_eq!(roles.len(), expected);
        Self::write_cells(queue, &self.color_texture, colors, self.tex_cols, self.tex_rows);
        Self::write_cells(queue, &self.role_texture, roles, self.tex_cols, self.tex_rows);
    }

    /// Copy one cell-grid plane into its texture.
    fn write_cells(
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        cells: &[u32],
        cols: u32,
        rows: u32,
    ) {
        let expected = (cols * rows) as usize;
        let len = cells.len().min(expected);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&cells[..len]),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(cols * 4),
                rows_per_image: Some(rows),
            },
            wgpu::Extent3d {
                width: cols,
                height: rows,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Render the compositing pass
    pub fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>) {
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.draw(0..3, 0..1); // Fullscreen triangle
    }
}

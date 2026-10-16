//! Quad renderer for gap-free cell backgrounds.
//!
//! Uploads the flat color array as a u32 texture (one texel per cell);
//! the shader indexes it by fragment position. A texture is used instead
//! of a storage buffer because WebGL2 fragment shaders cannot bind storage
//! buffers.

use bytemuck::{Pod, Zeroable};

/// Uniforms for background rendering.
///
/// The struct layout must match `shaders/background.wgsl`'s `Uniforms`:
/// three `vec2<f32>`, one `vec2<u32>`, one `vec4<f32>`, then four `f32`
/// effect magnitudes (64 bytes total).
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct BackgroundUniforms {
    pub screen_size: [f32; 2],
    pub cell_size: [f32; 2],
    pub grid_offset: [f32; 2],
    pub grid_dims: [u32; 2],
    /// Theme background: paints out-of-grid fragments.
    pub bg: [f32; 4],
    /// Panel-role luminance step, signed from the theme's darkness (0 = off).
    pub panel_shift: f32,
    /// Panel run edge luminance step (0 = off).
    pub panel_edge: f32,
    /// Selection run top-edge step (0 = off).
    pub selection_edge: f32,
    /// Cursor-neighbor blend factor (0 = off).
    pub cursor_halo: f32,
}

/// Knobs for [`resolve_effect_uniforms`]: the renderer fills these from
/// `crate::config::effects`, tests fill them with explicit values so no
/// test asserts a config constant.
#[derive(Clone, Copy, Debug)]
pub struct EffectKnobs {
    /// Master switch; false zeroes every slot.
    pub enabled: bool,
    /// Panel depth treatment on.
    pub panel: bool,
    /// Unsigned panel luminance magnitude.
    pub panel_shift: f32,
    /// Panel run edges on.
    pub panel_edge: bool,
    /// Unsigned panel edge magnitude.
    pub panel_edge_delta: f32,
    /// Selection top edge on.
    pub selection_edge: bool,
    /// Unsigned selection edge magnitude.
    pub selection_edge_delta: f32,
    /// Cursor halo on.
    pub cursor_halo: bool,
    /// Halo blend factor (0.0..=1.0), never signed.
    pub cursor_halo_alpha: f32,
}

/// Resolve the background-pass effect uniforms from the knobs and the
/// theme's darkness.
///
/// Pure over its inputs: disabled effects contribute exactly 0.0, which the
/// shader treats as the identity, and the luminance steps are signed here —
/// added on dark themes, subtracted on light ones — so the WGSL stays
/// theme-agnostic. Returns
/// `[panel_shift, panel_edge, selection_edge, cursor_halo]`.
pub fn resolve_effect_uniforms(knobs: EffectKnobs, is_dark: bool) -> [f32; 4] {
    if !knobs.enabled {
        return [0.0; 4];
    }
    let sign = if is_dark { 1.0 } else { -1.0 };
    [
        if knobs.panel {
            sign * knobs.panel_shift
        } else {
            0.0
        },
        if knobs.panel_edge {
            sign * knobs.panel_edge_delta
        } else {
            0.0
        },
        if knobs.selection_edge {
            sign * knobs.selection_edge_delta
        } else {
            0.0
        },
        if knobs.cursor_halo {
            knobs.cursor_halo_alpha
        } else {
            0.0
        },
    ]
}

/// Manages the background pass rendering
pub struct QuadRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform_buffer: wgpu::Buffer,
    color_texture: wgpu::Texture,
    color_view: wgpu::TextureView,
    role_texture: wgpu::Texture,
    role_view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    tex_cols: u32,
    tex_rows: u32,
}

impl QuadRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Background Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/background.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Background Bind Group Layout"),
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
                // Cell colors as a u32 texture (one texel per cell)
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Cell semantic roles as a u32 texture (one texel per cell,
                // the `CellRole` discriminant in the low byte)
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
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
            label: Some("Background Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Background Pipeline"),
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
            label: Some("Background Uniforms"),
            size: std::mem::size_of::<BackgroundUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 1x1 placeholders; sized to the grid on the first upload.
        let (color_texture, color_view) = Self::cell_texture(device, 1, 1, "Background Colors");
        let (role_texture, role_view) = Self::cell_texture(device, 1, 1, "Background Roles");

        let bind_group = Self::make_bind_group(
            device,
            &bind_group_layout,
            &uniform_buffer,
            &color_view,
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
            bind_group,
            tex_cols: 1,
            tex_rows: 1,
        }
    }

    /// One R32Uint cell-grid texture plus its view.
    fn cell_texture(
        device: &wgpu::Device,
        cols: u32,
        rows: u32,
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
            format: wgpu::TextureFormat::R32Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    /// Bind group over the uniforms and the two cell-grid textures.
    fn make_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        uniform_buffer: &wgpu::Buffer,
        color_view: &wgpu::TextureView,
        role_view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Background Bind Group"),
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
                    resource: wgpu::BindingResource::TextureView(role_view),
                },
            ],
        })
    }

    /// Update uniforms
    pub fn update_uniforms(&self, queue: &wgpu::Queue, uniforms: &BackgroundUniforms) {
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(uniforms));
    }

    /// Upload background colors and cell roles for a `cols` x `rows` grid.
    ///
    /// `colors` holds one packed ABGR `u32` per cell, `roles` one
    /// `CellRole` discriminant per cell. Both textures resize together when
    /// the grid dims change.
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
            (self.color_texture, self.color_view) =
                Self::cell_texture(device, self.tex_cols, self.tex_rows, "Background Colors");
            (self.role_texture, self.role_view) =
                Self::cell_texture(device, self.tex_cols, self.tex_rows, "Background Roles");

            // Recreate bind group
            self.bind_group = Self::make_bind_group(
                device,
                &self.bind_group_layout,
                &self.uniform_buffer,
                &self.color_view,
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

    /// Render the background pass
    pub fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>) {
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.draw(0..3, 0..1); // Fullscreen triangle
    }
}

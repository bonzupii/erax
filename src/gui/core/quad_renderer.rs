//! Quad renderer for gap-free cell backgrounds.
//!
//! Uploads the flat color array as a u32 texture (one texel per cell);
//! the shader indexes it by fragment position. A texture is used instead
//! of a storage buffer because WebGL2 fragment shaders cannot bind storage
//! buffers.

use bytemuck::{Pod, Zeroable};

/// Uniforms for background rendering
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct BackgroundUniforms {
    pub screen_size: [f32; 2],
    pub cell_size: [f32; 2],
    pub grid_offset: [f32; 2],
    pub grid_dims: [u32; 2],
}

/// Manages the background pass rendering
pub struct QuadRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform_buffer: wgpu::Buffer,
    color_texture: wgpu::Texture,
    color_view: wgpu::TextureView,
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

        // 1x1 placeholder; sized to the grid on the first upload.
        let color_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Background Colors"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let color_view = color_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Background Bind Group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&color_view),
                },
            ],
        });

        Self {
            pipeline,
            bind_group_layout,
            uniform_buffer,
            color_texture,
            color_view,
            bind_group,
            tex_cols: 1,
            tex_rows: 1,
        }
    }

    /// Update uniforms
    pub fn update_uniforms(&self, queue: &wgpu::Queue, uniforms: &BackgroundUniforms) {
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(uniforms));
    }

    /// Upload background colors for a `cols` x `rows` grid.
    pub fn upload_colors(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        colors: &[u32],
        cols: u32,
        rows: u32,
    ) {
        // Resize the texture when the grid dims change.
        if cols != self.tex_cols || rows != self.tex_rows {
            self.tex_cols = cols.max(1);
            self.tex_rows = rows.max(1);
            self.color_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Background Colors"),
                size: wgpu::Extent3d {
                    width: self.tex_cols,
                    height: self.tex_rows,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R32Uint,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.color_view = self
                .color_texture
                .create_view(&wgpu::TextureViewDescriptor::default());

            // Recreate bind group
            self.bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Background Bind Group"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniform_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&self.color_view),
                    },
                ],
            });
        }

        let expected = (self.tex_cols * self.tex_rows) as usize;
        debug_assert_eq!(colors.len(), expected);
        let len = colors.len().min(expected);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.color_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&colors[..len]),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.tex_cols * 4),
                rows_per_image: Some(self.tex_rows),
            },
            wgpu::Extent3d {
                width: self.tex_cols,
                height: self.tex_rows,
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

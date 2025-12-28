//! Glyph Atlas - Simple and correct implementation

use cosmic_text::fontdb::ID;
use rustc_hash::FxHashMap;

#[derive(Clone, Copy, Debug)]
pub struct AtlasSlot {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Hash, Eq, PartialEq, Clone, Copy, Debug)]
pub struct GlyphKey {
    pub font_id: ID,
    pub glyph_index: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct CachedGlyph {
    pub uv_x: f32,
    pub uv_y: f32,
    pub uv_w: f32,
    pub uv_h: f32,
    pub width: f32,
    pub height: f32,
    pub bearing_x: f32,
    pub bearing_y: f32,
}

struct PendingUpload {
    slot: AtlasSlot,
    pixels: Vec<u8>,
}

pub struct Atlas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    width: u32,
    height: u32,
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
    cache: FxHashMap<GlyphKey, CachedGlyph>,
    pending: Vec<PendingUpload>,
    generation: u64,
}

impl Atlas {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Glyph Atlas"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Self {
            texture,
            view,
            sampler,
            width,
            height,
            cursor_x: 0,
            cursor_y: 0,
            row_height: 0,
            cache: FxHashMap::default(),
            pending: Vec::new(),
            generation: 0,
        }
    }

    pub fn texture_view(&self) -> &wgpu::TextureView {
        &self.view
    }
    pub fn sampler(&self) -> &wgpu::Sampler {
        &self.sampler
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn get(&self, key: GlyphKey) -> Option<CachedGlyph> {
        self.cache.get(&key).copied()
    }

    pub fn is_full(&self) -> bool {
        self.cursor_y + 128 > self.height
    }

    pub fn reset(&mut self) {
        self.cache.clear();
        self.pending.clear();
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.row_height = 0;
        self.generation += 1;
    }

    pub fn allocate(&mut self, width: u32, height: u32) -> Option<AtlasSlot> {
        if width == 0 || height == 0 {
            return None;
        }

        if self.cursor_x + width > self.width {
            self.cursor_x = 0;
            self.cursor_y += self.row_height + 1;
            self.row_height = 0;
        }

        if self.cursor_y + height > self.height {
            return None;
        }

        let slot = AtlasSlot {
            x: self.cursor_x,
            y: self.cursor_y,
            width,
            height,
        };
        self.cursor_x += width + 1;
        self.row_height = self.row_height.max(height);
        Some(slot)
    }

    pub fn insert(
        &mut self,
        key: GlyphKey,
        slot: AtlasSlot,
        pixels: Vec<u8>,
        bearing_x: f32,
        bearing_y: f32,
    ) {
        let cached = CachedGlyph {
            uv_x: slot.x as f32 / self.width as f32,
            uv_y: slot.y as f32 / self.height as f32,
            uv_w: slot.width as f32 / self.width as f32,
            uv_h: slot.height as f32 / self.height as f32,
            width: slot.width as f32,
            height: slot.height as f32,
            bearing_x,
            bearing_y,
        };
        self.cache.insert(key, cached);
        self.pending.push(PendingUpload { slot, pixels });
    }

    pub fn flush(&mut self, queue: &wgpu::Queue) {
        for upload in self.pending.drain(..) {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: upload.slot.x,
                        y: upload.slot.y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &upload.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(upload.slot.width),
                    rows_per_image: Some(upload.slot.height),
                },
                wgpu::Extent3d {
                    width: upload.slot.width,
                    height: upload.slot.height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }
}

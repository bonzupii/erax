# GUI Stack API Research

Comprehensive documentation for erax's GUI rendering stack.

---

## Library Overview

| Library | Purpose | Version | Docs |
|---------|---------|---------|------|
| **winit** | Window creation, event loop | 0.30.x | [docs.rs/winit](https://docs.rs/winit) |
| **wgpu** | GPU rendering (WebGPU) | 27.x | [docs.rs/wgpu](https://docs.rs/wgpu) |
| **font-kit** | Font discovery & loading | 0.14.x | [docs.rs/font-kit](https://docs.rs/font-kit) |
| **fontdue** | Glyph rasterization | 0.9.x | [docs.rs/fontdue](https://docs.rs/fontdue) |
| **rustybuzz** | Text shaping (HarfBuzz) | 0.20.x | [docs.rs/rustybuzz](https://docs.rs/rustybuzz) |

---

## Terminal Emulator GPU Rendering Patterns

### Architecture Overview (Alacritty/WezTerm/Kitty Style)

```
┌────────────────────────────────────────────────────────────┐
│                     Terminal Grid                          │
│  ┌────┬────┬────┬────┬────┐                               │
│  │ H  │ e  │ l  │ l  │ o  │  ← Each cell = 1 instance    │
│  ├────┼────┼────┼────┼────┤                               │
│  │ W  │ o  │ r  │ l  │ d  │                               │
│  └────┴────┴────┴────┴────┘                               │
└────────────────────────────────────────────────────────────┘
                         ↓
┌────────────────────────────────────────────────────────────┐
│                    Glyph Atlas (GPU Texture)               │
│  ┌──────────────────────────────────────────┐             │
│  │ A B C D E F G H I J K ... 漢 コ ン ...  │             │
│  │ Pre-rasterized glyphs packed together   │             │
│  └──────────────────────────────────────────┘             │
└────────────────────────────────────────────────────────────┘
                         ↓
┌────────────────────────────────────────────────────────────┐
│              Instanced Rendering (1-2 draw calls)          │
│  • Each cell = 1 quad instance                             │
│  • Per-instance: position, fg/bg color, UV coords          │
│  • GPU samples glyph from atlas → draws to screen          │
└────────────────────────────────────────────────────────────┘
```

### Key Techniques

1. **Glyph Atlas**: Rasterize each unique glyph once, pack into single GPU texture
2. **Instanced Rendering**: One draw call for all cells (not one per character)
3. **Full Screen Redraw**: Faster than partial updates on modern GPUs
4. **Cell Grid**: Fixed-size cells, positions calculated from (row, col)

### Cell Instance Data (per-character)

```rust
#[repr(C)]
struct CellInstance {
    // Screen position (clip space or pixels)
    pos: [f32; 2],
    
    // UV coordinates into glyph atlas
    uv_origin: [f32; 2],
    uv_size: [f32; 2],
    
    // Colors (RGBA)
    fg_color: [f32; 4],
    bg_color: [f32; 4],
    
    // Cell span (1 for normal, 2 for CJK wide chars)
    cells_wide: f32,
}
```

### Shader Pattern (WGSL)

```wgsl
struct CellInstance {
    @location(0) pos: vec2<f32>,
    @location(1) uv_origin: vec2<f32>,
    @location(2) uv_size: vec2<f32>,
    @location(3) fg_color: vec4<f32>,
    @location(4) bg_color: vec4<f32>,
    @location(5) cells_wide: f32,
}

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_idx: u32,
    instance: CellInstance,
) -> VertexOutput {
    // Generate quad corners from vertex_index (0-5 for two triangles)
    let corner = get_quad_corner(vertex_idx);
    
    // Scale by cell size and cells_wide
    let cell_size = vec2<f32>(cell_width * instance.cells_wide, cell_height);
    let screen_pos = instance.pos + corner * cell_size;
    
    // UV for glyph atlas lookup
    let uv = instance.uv_origin + corner * instance.uv_size;
    
    return VertexOutput(screen_pos, uv, instance.fg_color, instance.bg_color);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let glyph_alpha = textureSample(glyph_atlas, sampler, in.uv).r;
    return mix(in.bg_color, in.fg_color, glyph_alpha);
}
```

### Box Drawing Characters

Terminal emulators often render box drawing (U+2500-U+257F) programmatically rather than from fonts:
- Ensures pixel-perfect alignment across cells
- Avoids anti-aliasing gaps at cell boundaries
- Common approach: detect box drawing range, render with lines/rectangles

---

## 1. winit (Windowing)

### Core Types

```rust
EventLoop           // Main event dispatcher
Window              // Native window handle
WindowEvent         // Input events (keyboard, mouse, resize)
ApplicationHandler  // Trait for event handling
```

### Event Loop Pattern

```rust
struct App { window: Option<Window> }

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.window = Some(event_loop.create_window(Window::default_attributes()).unwrap());
    }
    
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => { /* render */ }
            WindowEvent::KeyboardInput { event, .. } => { /* handle key */ }
            WindowEvent::Resized(size) => { /* resize surface */ }
            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Wait);  // Power-efficient
    event_loop.run_app(&mut App::default());
}
```

---

## 2. wgpu 27.x (GPU Rendering)

> **Note**: wgpu 28 has breaking changes (async enumerate_adapters, LoadOp::DontCare, MipmapFilterMode split). This documents v27.

### Initialization Chain

```rust
Instance → Adapter → Device + Queue → Surface
```

```rust
// Create instance (sync in v27)
let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
    backends: wgpu::Backends::PRIMARY,  // Vulkan/Metal/DX12
    ..Default::default()
});

// Create surface from window
let surface = instance.create_surface(window)?;

// Request adapter (async, use pollster::block_on)
let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
    power_preference: wgpu::PowerPreference::HighPerformance,
    compatible_surface: Some(&surface),
    force_fallback_adapter: false,
}))?;

// Request device (async)
let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
    label: Some("erax device"),
    required_features: wgpu::Features::empty(),
    required_limits: wgpu::Limits::downlevel_defaults(),
    memory_hints: wgpu::MemoryHints::Performance,
    experimental_features: wgpu::ExperimentalFeatures::default(),
    trace: wgpu::Trace::default(),
}))?;
```

### Core Types (v27)

| Type | Purpose |
|------|---------|
| `Instance` | Entry point, creates adapters/surfaces |
| `Adapter` | Physical GPU handle |
| `Device` | Logical GPU, creates resources |
| `Queue` | Command submission |
| `Surface` | Window render target |
| `Buffer` | GPU memory |
| `Texture` | 2D image data |
| `TextureView` | View into texture for binding |
| `Sampler` | Texture sampling parameters |
| `BindGroup` | Resource bindings for shaders |
| `RenderPipeline` | Shader + render state |

### Texture Upload (v27 API)

```rust
// wgpu 27 uses TexelCopyTextureInfo and TexelCopyBufferLayout
queue.write_texture(
    wgpu::TexelCopyTextureInfo {
        texture: &texture,
        mip_level: 0,
        origin: wgpu::Origin3d { x, y, z: 0 },
        aspect: wgpu::TextureAspect::All,
    },
    &bitmap_data,
    wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(width),
        rows_per_image: None,  // Single layer
    },
    wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
);
```

### Render Loop (v27)

```rust
let output = surface.get_current_texture()?;
let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
{
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("render pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(clear_color),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(&pipeline);
    pass.set_bind_group(0, &bind_group, &[]);
    pass.set_vertex_buffer(0, vertex_buffer.slice(..));
    pass.draw(0..vertex_count, 0..instance_count);
}

queue.submit([encoder.finish()]);
output.present();
```

### Shaders (WGSL - default in v27)

---

## 3. font-kit (Font Discovery)

### Handle Enum - **CRITICAL FOR TTC FONTS**

```rust
pub enum Handle {
    Path {
        path: PathBuf,
        font_index: u32,  // TTC collection index
    },
    Memory {
        bytes: Arc<Vec<u8>>,
        font_index: u32,  // TTC collection index
    },
}
```

> **⚠️ CRITICAL**: `font_index` MUST be passed to fontdue's `FontSettings.collection_index`!

### Font Discovery

```rust
use font_kit::source::SystemSource;
use font_kit::family_name::FamilyName;
use font_kit::properties::Properties;

let source = SystemSource::new();
let handle = source.select_best_match(
    &[FamilyName::Title("Noto Sans CJK JP".to_string())],
    &Properties::new(),
)?;

// Extract font_index for TTC fonts
let font_index = match &handle {
    Handle::Path { font_index, .. } => *font_index,
    Handle::Memory { font_index, .. } => *font_index,
};

let font = handle.load()?;
let data = font.copy_font_data()?.to_vec();  // Full TTC file bytes
```

### Glyph Check (font-kit)

```rust
let glyph_id = font.glyph_for_char('コ');  // Option<u32>
```

---

## 4. fontdue (Rasterization)

### FontSettings - **MUST USE font_index**

```rust
pub struct FontSettings {
    pub collection_index: u32,    // TTC font index (MUST match font-kit's font_index)
    pub scale: f32,               // Optimal render size (default: 40.0)
    pub load_substitutions: bool, // Load ligatures (default: true)
}
```

### Font Loading

```rust
// CORRECT: Pass font_index from font-kit
let settings = FontSettings {
    collection_index: font_index,  // From Handle.font_index
    ..Default::default()
};
let font = Font::from_bytes(&data, settings)?;
```

### Glyph Check & Rasterization

```rust
// Check if font has glyph (0 = missing)
let glyph_id = font.lookup_glyph_index('コ');
if glyph_id == 0 {
    // Font doesn't have this character
}

// Rasterize
let (metrics, bitmap) = font.rasterize('A', 16.0);
// metrics.width, metrics.height - bitmap dimensions
// bitmap - Vec<u8> grayscale coverage (0-255)
```

### Metrics Struct

```rust
pub struct Metrics {
    pub xmin: i32,
    pub ymin: i32,
    pub width: usize,
    pub height: usize,
    pub advance_width: f32,
    pub advance_height: f32,
}
```

---

## 5. rustybuzz (Text Shaping)

### Face Creation - **ALSO NEEDS font_index**

```rust
// Create Face with TTC index
let face = Face::from_slice(&font_data, font_index)?;
```

### Shaping

```rust
let mut buffer = UnicodeBuffer::new();
buffer.push_str("ffi");  // Will become ligature

let glyph_buffer = rustybuzz::shape(&face, &[], buffer);

for (info, pos) in glyph_buffer.glyph_infos().iter()
    .zip(glyph_buffer.glyph_positions()) 
{
    // info.glyph_id - glyph to rasterize (pass to fontdue)
    // info.cluster - maps back to original text position
    // pos.x_advance - horizontal movement (in font units)
    // pos.y_advance - vertical movement
    // pos.x_offset - horizontal offset
    // pos.y_offset - vertical offset
}
```

### Performance: ShapePlan

```rust
// For repeated shaping with same font
let plan = ShapePlan::new(&face, Direction::LeftToRight, None, None, &[]);
let glyph_buffer = shape_with_plan(&face, &plan, buffer);
```

---

## Noto CJK TTC Structure

| Index | Font Face | Region |
|-------|-----------|--------|
| 0 | Noto Sans CJK SC | Simplified Chinese |
| 1 | Noto Sans CJK TC | Traditional Chinese |
| 2 | Noto Sans CJK HK | Hong Kong |
| 3 | Noto Sans CJK JP | Japanese |
| 4 | Noto Sans CJK KR | Korean |

All faces contain ALL CJK+Kana+Hangul characters, but with region-specific glyph shapes.

---

## Current Bug: CJK Characters Not Rendering

### Root Cause

```rust
// CURRENT CODE (BROKEN)
let font_kit_font = handle.load()?;
let data = font_kit_font.copy_font_data()?.to_vec();
let font = Font::from_bytes(&data, FontSettings::default())?;  // Uses collection_index: 0
```

font-kit selects "Noto Sans CJK JP" and returns `Handle { font_index: 3 }`, but we ignore it and load index 0 (Simplified Chinese).

### Fix

```rust
// FIXED CODE
let font_index = match &handle {
    Handle::Path { font_index, .. } => *font_index,
    Handle::Memory { font_index, .. } => *font_index,
};

let font_kit_font = handle.load()?;
let data = font_kit_font.copy_font_data()?.to_vec();

let settings = FontSettings {
    collection_index: font_index,
    ..Default::default()
};
let font = Font::from_bytes(&data, settings)?;
```

---

## Data Flow Summary

```
User Request: "Noto Sans CJK JP"
        ↓
font-kit: select_best_match() → Handle { font_index: 3 }
        ↓
font-kit: load().copy_font_data() → Vec<u8> (entire TTC)
        ↓
fontdue: Font::from_bytes(data, { collection_index: 3 })
        ↓
rustybuzz: Face::from_slice(data, 3)  (for shaping)
        ↓
fontdue: font.rasterize() or font.rasterize_indexed()
        ↓
wgpu: queue.write_texture() → atlas
        ↓
wgpu: render_pass.draw() → screen
```

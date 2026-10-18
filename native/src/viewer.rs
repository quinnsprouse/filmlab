use std::sync::Arc;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSWindowOrderingMode};
use objc2_core_graphics::{CGColor, CGColorSpace};
use objc2_quartz_core::{CALayer, CAMetalLayer};

use crate::camera::{Camera, Native, Rect, Region, Size, rect_of};
use crate::glass::renderer_view;
use crate::gpu::Gpu;

const SHADER: &str = r#"
struct Quad { rect: vec4f, background: vec4f }
@group(0) @binding(0) var photo: texture_2d<f32>;
@group(0) @binding(1) var pick: sampler;
@group(0) @binding(2) var<uniform> quad: Quad;

struct Out { @builtin(position) at: vec4f, @location(0) uv: vec2f }

@vertex fn vertex(@builtin(vertex_index) i: u32) -> Out {
  let corner = vec2f(f32((i == 1u) || (i == 2u) || (i == 4u)), f32((i == 2u) || (i == 4u) || (i == 5u)));
  let at = mix(quad.rect.xy, quad.rect.zw, corner);
  return Out(vec4f(at, 0.0, 1.0), corner);
}

@fragment fn fragment(input: Out) -> @location(0) vec4f {
  return vec4f(textureSampleLevel(photo, pick, input.uv, 0.0).rgb, 1.0);
}
"#;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    Fit,
    Detail,
}

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub region: Region,
}

struct Shown {
    region: Region,
    size: (u32, u32),
    view: wgpu::TextureView,
    uniform: wgpu::Buffer,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Scene {
    pub camera: Camera,
    pub native: Native,
    pub stage: Rect,
    pub drawable: (u32, u32),
    pub background: [f64; 3],
}

struct Target {
    layer: Retained<CAMetalLayer>,
    surface: wgpu::Surface<'static>,
    configured: Option<(u32, u32)>,
}

pub struct Viewer {
    gpu: Arc<Gpu>,
    view: Retained<NSView>,
    target: Option<Target>,
    parked: bool,
    pipeline: wgpu::RenderPipeline,
    binds: wgpu::BindGroupLayout,
    nearest: wgpu::Sampler,
    linear: wgpu::Sampler,
    fit: Option<Shown>,
    detail: Option<Shown>,
    drawn: Option<Scene>,
    dirty: bool,
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

impl Viewer {
    pub fn new(window: &gpui_kit::Window, gpu: Arc<Gpu>) -> Option<Viewer> {
        let mtm = MainThreadMarker::new()?;
        let gpui = renderer_view(window)?;
        let parent = unsafe { gpui.superview() }?;
        let view = NSView::initWithFrame(NSView::alloc(mtm), parent.bounds());
        let target = Target::new(&gpu, &view)?;
        view.setWantsLayer(true);
        view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
        parent.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, None);
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("viewer"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let binds = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&binds)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewer"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vertex"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fragment"), compilation_options: Default::default(), targets: &[Some(FORMAT.into())] }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = |filter| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: filter,
                min_filter: filter,
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                ..Default::default()
            })
        };
        let (nearest, linear) = (sampler(wgpu::FilterMode::Nearest), sampler(wgpu::FilterMode::Linear));
        Some(Viewer { gpu, view, target: Some(target), parked: false, pipeline, binds, nearest, linear, fit: None, detail: None, drawn: None, dirty: true })
    }

    pub fn show(&mut self, layer: Layer, frame: Option<Frame>) {
        let shown = frame.map(|f| {
            let texture = crate::gpu::texture_rgba(&self.gpu, f.width, f.height, &f.rgba);
            let uniform = self.gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 32, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            Shown { region: f.region, size: (f.width, f.height), view: texture.create_view(&Default::default()), uniform }
        });
        match layer {
            Layer::Fit => self.fit = shown,
            Layer::Detail => self.detail = shown,
        }
        self.dirty = true;
    }

    pub fn has(&self, layer: Layer) -> bool {
        match layer {
            Layer::Fit => self.fit.is_some(),
            Layer::Detail => self.detail.is_some(),
        }
    }

    // why: in Browse the viewer shows only the background; a plain layer filled with it stands in for the Metal
    // layer, whose window-sized drawables go with it
    pub fn park(&mut self, background: [f64; 3]) {
        if self.parked {
            return;
        }
        let plain = CALayer::new();
        let [r, g, b] = background;
        plain.setBackgroundColor(Some(&CGColor::new_srgb(r, g, b, 1.0)));
        self.view.setLayer(Some(&plain));
        self.target = None;
        self.fit = None;
        self.detail = None;
        self.drawn = None;
        self.parked = true;
    }

    pub fn unpark(&mut self) {
        self.parked = false;
    }

    pub fn draw(&mut self, scene: Scene) {
        if self.parked || (!self.dirty && self.drawn == Some(scene)) {
            return;
        }
        if self.target.is_none() {
            self.target = Target::new(&self.gpu, &self.view);
        }
        let Some(target) = self.target.as_mut() else { return };
        if let Some(window) = self.view.window() {
            target.layer.setContentsScale(window.backingScaleFactor());
        }
        // why: before a photo's first frame the layer only shows the background, which a 1×1 drawable stretched
        // over it shows as well
        let empty = self.fit.is_none() && self.detail.is_none();
        target.configure(&self.gpu, if empty { (1, 1) } else { scene.drawable });
        let output = match target.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated => {
                target.configured = None;
                self.dirty = true;
                return;
            }
            _ => return,
        };
        let target = output.texture.create_view(&Default::default());
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        let binds: Vec<wgpu::BindGroup> = [&self.fit, &self.detail].into_iter().flatten().map(|shown| self.bind(shown, scene)).collect();
        {
            let [r, g, b] = scene.background;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a: 1.0 }), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            for bind in &binds {
                pass.set_bind_group(0, bind, &[]);
                pass.draw(0..6, 0..1);
            }
        }
        self.gpu.queue.submit([encoder.finish()]);
        self.gpu.queue.present(output);
        // why: wgpu frees dropped textures and spent upload space only when the device is polled
        let _ = self.gpu.device.poll(wgpu::PollType::Poll);
        self.drawn = Some(scene);
        self.dirty = false;
    }

    fn bind(&self, shown: &Shown, scene: Scene) -> wgpu::BindGroup {
        let viewport = Size { width: scene.stage.width, height: scene.stage.height };
        let r = rect_of(scene.camera, shown.region, scene.native, viewport);
        let (x, y) = (scene.stage.x + r.x, scene.stage.y + r.y);
        let (dw, dh) = (f64::from(scene.drawable.0.max(1)), f64::from(scene.drawable.1.max(1)));
        let ndc = |px: f64, py: f64| [(px / dw * 2.0 - 1.0) as f32, (1.0 - py / dh * 2.0) as f32];
        let [x0, y0] = ndc(x, y);
        let [x1, y1] = ndc(x + r.width, y + r.height);
        let words = [x0, y0, x1, y1, 0.0, 0.0, 0.0, 0.0];
        self.gpu.queue.write_buffer(&shown.uniform, 0, bytemuck::cast_slice(&words));
        // why: at 1:1 each texel lands on one device pixel, so nearest keeps the engine's exact values
        let exact = (r.width - f64::from(shown.size.0)).abs() < 0.5 && (r.height - f64::from(shown.size.1)).abs() < 0.5;
        let sampler = if exact { &self.nearest } else { &self.linear };
        self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.binds,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&shown.view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: shown.uniform.as_entire_binding() },
            ],
        })
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        self.view.removeFromSuperview();
    }
}

impl Target {
    fn new(gpu: &Gpu, view: &NSView) -> Option<Target> {
        let layer = CAMetalLayer::new();
        view.setLayer(Some(&layer));
        let target = wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(Retained::as_ptr(&layer) as *mut std::ffi::c_void);
        let surface = unsafe { gpu.instance.create_surface_unsafe(target) }.ok()?;
        Some(Target { layer, surface, configured: None })
    }

    fn configure(&mut self, gpu: &Gpu, (w, h): (u32, u32)) {
        if self.configured == Some((w, h)) || w == 0 || h == 0 {
            return;
        }
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: FORMAT,
            width: w,
            height: h,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 1,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Srgb,
        };
        self.surface.configure(&gpu.device, &config);
        // why: the engine's pixels are sRGB; naming the layer's space lets macOS colour-manage them to the
        // display as the browser did, rather than showing them as the panel's native (P3) values
        let srgb = CGColorSpace::with_name(Some(unsafe { objc2_core_graphics::kCGColorSpaceSRGB }));
        self.layer.setColorspace(srgb.as_deref());
        self.configured = Some((w, h));
    }
}

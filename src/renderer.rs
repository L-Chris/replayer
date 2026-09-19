use crate::player::VideoFrame;
use anyhow::Result;
use eframe::{egui, egui_wgpu, wgpu};
use ffmpeg::{format::Pixel, frame, software};
use ffmpeg_next as ffmpeg;

/// The UI consumes a texture/aspect ratio; pixel conversion stays here.
pub struct VideoRenderer {
    gpu: Option<GpuVideo>,
    cpu_texture: Option<egui::TextureHandle>,
    scaler: Option<software::scaling::Context>,
    converted: frame::Video,
    staging: Vec<u8>,
    current: Option<(egui::TextureId, f32)>,
}
impl VideoRenderer {
    pub fn new(state: Option<egui_wgpu::RenderState>) -> Self {
        Self {
            gpu: state.map(GpuVideo::new),
            cpu_texture: None,
            scaler: None,
            converted: frame::Video::empty(),
            staging: Vec::new(),
            current: None,
        }
    }
    pub fn clear(&mut self) {
        self.current = None;
    }
    pub fn current(&self) -> Option<(egui::TextureId, f32)> {
        self.current
    }
    pub fn upload(&mut self, ctx: &egui::Context, frame: &VideoFrame) -> Result<()> {
        let source = frame.image();
        let aspect = frame.aspect();
        if matches!(
            source.format(),
            Pixel::YUV420P | Pixel::YUVJ420P | Pixel::NV12
        ) && let Some(gpu) = &mut self.gpu
            && source.width() <= gpu.state.device.limits().max_texture_dimension_2d
            && source.height() <= gpu.state.device.limits().max_texture_dimension_2d
        {
            self.current = Some((gpu.upload(source), aspect));
            return Ok(());
        }
        let w = source.width();
        let h = source.height();
        let replace = self.scaler.as_ref().is_none_or(|s| {
            s.input().format != source.format() || s.input().width != w || s.input().height != h
        });
        if replace {
            self.scaler = Some(software::scaling::Context::get(
                source.format(),
                w,
                h,
                Pixel::RGBA,
                w,
                h,
                software::scaling::Flags::BILINEAR,
            )?);
            self.converted = frame::Video::empty();
        }
        self.scaler
            .as_mut()
            .unwrap()
            .run(source, &mut self.converted)?;
        self.staging.resize(w as usize * h as usize * 4, 0);
        for (row, dst) in self.staging.chunks_exact_mut(w as usize * 4).enumerate() {
            let offset = row * self.converted.stride(0);
            dst.copy_from_slice(&self.converted.data(0)[offset..offset + w as usize * 4]);
        }
        let image =
            egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &self.staging);
        if let Some(texture) = &mut self.cpu_texture {
            texture.set(image, egui::TextureOptions::LINEAR);
        } else {
            self.cpu_texture =
                Some(ctx.load_texture("video-cpu", image, egui::TextureOptions::LINEAR));
        }
        self.current = Some((self.cpu_texture.as_ref().unwrap().id(), aspect));
        Ok(())
    }
}

struct Images {
    key: (u32, u32, bool),
    planes: [wgpu::Texture; 3],
    target: wgpu::Texture,
    bind: wgpu::BindGroup,
    id: egui::TextureId,
}
struct GpuVideo {
    state: egui_wgpu::RenderState,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    images: Option<Images>,
}
impl GpuVideo {
    fn new(state: egui_wgpu::RenderState) -> Self {
        let device = &state.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("video-yuv"),
            source: wgpu::ShaderSource::Wgsl(include_str!("video.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("video-yuv"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("video-matrix"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            state,
            pipeline,
            uniform,
            sampler,
            images: None,
        }
    }
    fn upload(&mut self, frame: &frame::Video) -> egui::TextureId {
        let w = frame.width();
        let h = frame.height();
        let nv12 = frame.format() == Pixel::NV12;
        let key = (w, h, nv12);
        if self.images.as_ref().is_none_or(|i| i.key != key) {
            if let Some(old) = self.images.take() {
                self.state.renderer.write().free_texture(&old.id);
            }
            let make = |width, height, format, usage| {
                self.state.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("video-surface"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            };
            let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
            let planes = [
                make(w, h, wgpu::TextureFormat::R8Unorm, usage),
                make(
                    w.div_ceil(2),
                    h.div_ceil(2),
                    if nv12 {
                        wgpu::TextureFormat::Rg8Unorm
                    } else {
                        wgpu::TextureFormat::R8Unorm
                    },
                    usage,
                ),
                make(
                    w.div_ceil(2),
                    h.div_ceil(2),
                    wgpu::TextureFormat::R8Unorm,
                    usage,
                ),
            ];
            let target = make(
                w,
                h,
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_SRC,
            );
            let views = planes
                .each_ref()
                .map(|t| t.create_view(&Default::default()));
            let bind = self
                .state
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("video-planes"),
                    layout: &self.pipeline.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&views[0]),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&views[1]),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&views[2]),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: self.uniform.as_entire_binding(),
                        },
                    ],
                });
            let id = self.state.renderer.write().register_native_texture(
                &self.state.device,
                &target.create_view(&Default::default()),
                wgpu::FilterMode::Linear,
            );
            self.images = Some(Images {
                key,
                planes,
                target,
                bind,
                id,
            });
        }
        let images = self.images.as_ref().unwrap();
        for plane in 0..if nv12 { 2 } else { 3 } {
            self.state.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &images.planes[plane],
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                frame.data(plane),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(frame.stride(plane) as u32),
                    rows_per_image: None,
                },
                images.planes[plane].size(),
            );
        }
        let matrix = color_matrix(frame);
        self.state
            .queue
            .write_buffer(&self.uniform, 0, bytemuck::cast_slice(&matrix));
        let mut encoder = self
            .state
            .device
            .create_command_encoder(&Default::default());
        let view = images.target.create_view(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("video-convert"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &images.bind, &[]);
            pass.draw(0..3, 0..1);
        }
        self.state.queue.submit([encoder.finish()]);
        images.id
    }
}
impl Drop for GpuVideo {
    fn drop(&mut self) {
        if let Some(images) = &self.images {
            self.state.renderer.write().free_texture(&images.id);
        }
    }
}
fn color_matrix(frame: &frame::Video) -> [[f32; 4]; 4] {
    use ffmpeg::util::color::{Range, Space};
    let (kr, kb) = match frame.color_space() {
        Space::BT709 => (0.2126, 0.0722),
        Space::BT2020NCL | Space::BT2020CL => (0.2627, 0.0593),
        Space::Unspecified if frame.height() > 576 => (0.2126, 0.0722),
        _ => (0.299, 0.114),
    };
    let full = frame.color_range() == Range::JPEG || frame.format() == Pixel::YUVJ420P;
    let y = if full { 1.0 } else { 255.0 / 219.0 };
    let c = if full { 1.0 } else { 255.0 / 224.0 };
    let offset = if full { 0.0 } else { -16.0 / 255.0 * y };
    let rv = 2.0 * (1.0 - kr) * c;
    let bu = 2.0 * (1.0 - kb) * c;
    let gu = -2.0 * kb * (1.0 - kb) / (1.0 - kr - kb) * c;
    let gv = -2.0 * kr * (1.0 - kr) / (1.0 - kr - kb) * c;
    let center = 128.0 / 255.0;
    [
        [y, 0.0, rv, offset - rv * center],
        [y, gu, gv, offset - (gu + gv) * center],
        [y, bu, 0.0, offset - bu * center],
        [
            if frame.format() == Pixel::NV12 {
                1.0
            } else {
                0.0
            },
            0.0,
            0.0,
            0.0,
        ],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn limited_range_black_and_white() {
        let f = frame::Video::new(Pixel::YUV420P, 16, 16);
        let matrix = color_matrix(&f);
        for row in &matrix[..3] {
            let convert = |y| row[0] * y + (row[1] + row[2]) * 128.0 / 255.0 + row[3];
            assert!(convert(16.0 / 255.0).abs() < 0.0001);
            assert!((convert(235.0 / 255.0) - 1.0).abs() < 0.0001);
        }
    }

    #[test]
    #[ignore = "requires a GPU adapter; run with cargo test gpu_yuv_readback -- --ignored"]
    fn gpu_yuv_readback() {
        use std::{
            future::Future,
            sync::Arc,
            task::{Context, Poll, Wake, Waker},
        };
        struct WakeThread(std::thread::Thread);
        impl Wake for WakeThread {
            fn wake(self: Arc<Self>) {
                self.0.unpark();
            }
        }
        fn block_on<F: Future>(future: F) -> F::Output {
            let waker = Waker::from(Arc::new(WakeThread(std::thread::current())));
            let mut cx = Context::from_waker(&waker);
            let mut f = std::pin::pin!(future);
            loop {
                if let Poll::Ready(value) = f.as_mut().poll(&mut cx) {
                    return value;
                }
                std::thread::park();
            }
        }
        let instance = wgpu::Instance::default();
        let adapter = block_on(instance.request_adapter(&Default::default())).unwrap();
        println!("GPU readback adapter: {:?}", adapter.get_info());
        let (device, queue) = block_on(adapter.request_device(&Default::default())).unwrap();
        let renderer =
            egui_wgpu::Renderer::new(&device, wgpu::TextureFormat::Rgba8Unorm, Default::default());
        let state = egui_wgpu::RenderState {
            instance,
            adapter,
            available_adapters: vec![],
            device,
            queue,
            target_format: wgpu::TextureFormat::Rgba8Unorm,
            renderer: Arc::new(egui::mutex::RwLock::new(renderer)),
            surface_config: egui_wgpu::SurfaceConfig::LOW_LATENCY,
        };
        let mut gpu = GpuVideo::new(state);
        for (format, y, u, v, expected) in [
            (Pixel::YUV420P, 16, 128, 128, [0, 0, 0]),
            (Pixel::YUV420P, 235, 128, 128, [255, 255, 255]),
            (Pixel::YUV420P, 81, 90, 240, [254, 0, 0]),
            (Pixel::NV12, 81, 90, 240, [254, 0, 0]),
            (Pixel::NV12, 235, 128, 128, [255, 255, 255]),
        ] {
            let mut frame = frame::Video::new(format, 7, 5);
            frame.data_mut(0).fill(y);
            if format == Pixel::NV12 {
                for pair in frame.data_mut(1).chunks_exact_mut(2) {
                    pair.copy_from_slice(&[u, v]);
                }
            } else {
                frame.data_mut(1).fill(u);
                frame.data_mut(2).fill(v);
            }
            gpu.upload(&frame);
            let buffer = gpu.state.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("readback"),
                size: 256 * 5,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut encoder = gpu.state.device.create_command_encoder(&Default::default());
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &gpu.images.as_ref().unwrap().target,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: None,
                    },
                },
                wgpu::Extent3d {
                    width: 7,
                    height: 5,
                    depth_or_array_layers: 1,
                },
            );
            gpu.state.queue.submit([encoder.finish()]);
            let (tx, rx) = std::sync::mpsc::channel();
            buffer.map_async(wgpu::MapMode::Read, .., move |result| {
                tx.send(result).unwrap();
            });
            gpu.state
                .device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            rx.recv().unwrap().unwrap();
            let bytes = buffer.get_mapped_range(..).unwrap();
            for row in 0..5 {
                for col in 0..7 {
                    for channel in 0..3 {
                        let actual = bytes[row * 256 + col * 4 + channel];
                        assert!(
                            (actual as i32 - expected[channel]).abs() <= 2,
                            "{format:?} pixel {row},{col} channel {channel}: {actual} vs {}",
                            expected[channel]
                        );
                    }
                }
            }
        }
    }
}

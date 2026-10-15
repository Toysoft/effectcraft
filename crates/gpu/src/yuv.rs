//! Export frames converted on the device to the H.264 encoder's input (8-bit BT.709 limited
//! range 4:2:0, see `shaders/yuv.wgsl`) and read back asynchronously: 12 MB per 4K frame
//! instead of 133 MB of RGBA f32, and the worker records the next frame while the copy runs.

use std::sync::{Arc, Mutex};

use wgpu::util::DeviceExt;

use crate::context::{Enc, GpuContext, GpuImage};

pub(crate) struct YuvPipe {
    bgl: wgpu::BindGroupLayout,
    pipe: wgpu::ComputePipeline,
    /// Storage buffers the kernel writes, by size.
    outs: Mutex<Vec<wgpu::Buffer>>,
    /// Mappable staging buffers, returned by the map callbacks.
    staging: Arc<Mutex<Vec<wgpu::Buffer>>>,
}

/// Buffers kept for reuse (each ring).
const RING: usize = 8;

impl YuvPipe {
    pub(crate) fn new(device: &wgpu::Device) -> YuvPipe {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("yuv420"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/yuv.wgsl").into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yuv420"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("yuv420"), bind_group_layouts: &[Some(&bgl)], immediate_size: 0 });
        let pipe = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("yuv420"),
            layout: Some(&layout),
            module: &module,
            entry_point: Some("yuv420"),
            compilation_options: Default::default(),
            cache: None,
        });
        YuvPipe { bgl, pipe, outs: Mutex::new(vec![]), staging: Arc::new(Mutex::new(vec![])) }
    }
}

fn take(ring: &Mutex<Vec<wgpu::Buffer>>, size: u64, make: impl FnOnce() -> wgpu::Buffer) -> wgpu::Buffer {
    let found = ring.lock().ok().and_then(|mut r| r.iter().position(|b| b.size() == size).map(|i| r.swap_remove(i)));
    found.unwrap_or_else(make)
}

fn give(ring: &Mutex<Vec<wgpu::Buffer>>, b: wgpu::Buffer) {
    if let Ok(mut r) = ring.lock()
        && r.len() < RING
    {
        r.push(b);
    }
}

/// Convert `img` (`w` a multiple of 8, `h` even) over `bg`, submit, and hand the planes
/// (Y `w×h`, then U and V `w/2 × h/2`) to `done` from a later device poll.
pub(crate) fn convert(e: &mut Enc, yp: &YuvPipe, img: &GpuImage, bg: [f32; 3], done: Box<dyn FnOnce(Option<Vec<u8>>) + Send>) {
    let g: &GpuContext = e.g;
    let (w, h) = (img.width, img.height);
    let cw = w / 2;
    let ywords = w * h / 4;
    let size = (w as u64 * h as u64) + 2 * (cw as u64 * (h / 2) as u64);
    let out = take(&yp.outs, size, || {
        g.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yuv420 out"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        })
    });
    let stg = take(&yp.staging, size, || {
        g.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yuv420 staging"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    });
    let mut params = [0u8; 32];
    for (i, v) in [w, h, cw, ywords].into_iter().enumerate() {
        params[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    for (i, v) in [bg[0], bg[1], bg[2], 1.0].into_iter().enumerate() {
        params[16 + i * 4..16 + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    let ub =
        g.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("yuv420 params"), contents: &params, usage: wgpu::BufferUsages::UNIFORM });
    let view = img.texture.create_view(&Default::default());
    let bg_ = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("yuv420"),
        layout: &yp.bgl,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: ub.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 2, resource: out.as_entire_binding() },
        ],
    });
    {
        let enc = e.encoder();
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("yuv420"), timestamp_writes: None });
            pass.set_pipeline(&yp.pipe);
            pass.set_bind_group(0, &bg_, &[]);
            pass.dispatch_workgroups((w / 8).div_ceil(8), (h / 2).div_ceil(8), 1);
        }
        enc.copy_buffer_to_buffer(&out, 0, &stg, 0, size);
    }
    e.submit();
    // Later work on the queue runs after this copy: the storage buffer is free to reuse.
    give(&yp.outs, out);
    let ring = yp.staging.clone();
    let b = stg.clone();
    // Through the context's bounded readbacks (deadline, retirement on device loss).
    g.map_readback(&stg, size as usize, size as usize, 1, move |r| {
        give(&ring, b);
        done(r.map_err(|e| log::error!(target: "effectcraft_gpu", "yuv420 readback: {e}")).ok());
    });
}

#[cfg(test)]
mod tests {
    use effectcraft_raster::Image;

    /// export's `to_rgba8_over` + `rgba_to_yuv420` (crates/export/src/encode.rs), verbatim.
    fn cpu(img: &Image, bg: [f32; 3]) -> Vec<u8> {
        let rgba = img.to_rgba8_over(bg);
        let (w, h) = (img.width as usize, img.height as usize);
        let cw = w / 2;
        let mut y = vec![0u8; w * h];
        let mut u = vec![0u8; cw * (h / 2)];
        let mut v = vec![0u8; cw * (h / 2)];
        for cy in 0..h / 2 {
            let mut acc = vec![(0f32, 0f32, 0f32); cw];
            for dy in 0..2 {
                let src = &rgba[(cy * 2 + dy) * w * 4..][..w * 4];
                for x in 0..w {
                    let (r, g, b) = (src[x * 4] as f32 / 255.0, src[x * 4 + 1] as f32 / 255.0, src[x * 4 + 2] as f32 / 255.0);
                    let l = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                    y[(cy * 2 + dy) * w + x] = (16.0 + 219.0 * l).round().clamp(1.0, 254.0) as u8;
                    let a = &mut acc[x / 2];
                    a.0 += (b - l) / 1.8556;
                    a.1 += (r - l) / 1.5748;
                    a.2 += 1.0;
                }
            }
            for (cx, a) in acc.iter().enumerate() {
                u[cy * cw + cx] = (128.0 + 224.0 * a.0 / a.2).round().clamp(1.0, 254.0) as u8;
                v[cy * cw + cx] = (128.0 + 224.0 * a.1 / a.2).round().clamp(1.0, 254.0) as u8;
            }
        }
        [y, u, v].concat()
    }

    #[test]
    fn yuv420_matches_cpu() {
        let Some(g) = crate::tests::gpu() else { return };
        let (w, h) = (264u32, 150u32);
        let mut img = Image::new(w, h);
        let mut s = 0x9e3779b9u32;
        let mut rnd = || {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            s as f32 / u32::MAX as f32
        };
        for (i, p) in img.data.iter_mut().enumerate() {
            let a = if i % 7 == 0 { 1.0 } else { rnd() };
            // premultiplied, a few out of range, plus exact 8-bit levels (rounding ties)
            *p = if i % 5 == 0 {
                let q = (i % 256) as f32 / 255.0;
                [q, 1.0 - q, q * 0.5, 1.0]
            } else {
                [rnd() * a * 1.1 - 0.02, rnd() * a, rnd() * a, a]
            };
        }
        let bg = [0.05, 0.1, 0.2];
        let want = cpu(&img, bg);
        let src = g.ctx.upload_image(&img).expect("upload");
        let yp = g.ctx.yuv.get_or_init(|| super::YuvPipe::new(&g.ctx.device));
        let got = std::sync::Arc::new(std::sync::Mutex::new(None));
        let got2 = got.clone();
        let mut e = crate::context::Enc::new(&g.ctx);
        super::convert(&mut e, yp, &src, bg, Box::new(move |b| *got2.lock().unwrap() = Some(b)));
        drop(e);
        for _ in 0..200 {
            // Straight to the device: a short wait timing out is normal and must not retire the context.
            let _ = g.ctx.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(std::time::Duration::from_millis(50)) });
            if got.lock().unwrap().is_some() {
                break;
            }
        }
        let got = got.lock().unwrap().take().flatten().expect("readback");
        assert_eq!(got.len(), want.len());
        let diff: Vec<usize> = (0..got.len()).filter(|&i| got[i] != want[i]).collect();
        let max = diff.iter().map(|&i| (got[i] as i32 - want[i] as i32).abs()).max().unwrap_or(0);
        eprintln!("yuv420: {} of {} bytes differ (max {max}) on {}", diff.len(), got.len(), g.ctx.name);
        assert!(diff.is_empty(), "first diffs at {:?}", &diff[..diff.len().min(10)]);
    }
}

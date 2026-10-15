//! The streaming scheduler (an export with an accelerator attached) produces the same movie as
//! a CPU export, in frame order, whether the GPU renders frames or declines them.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use effectcraft_color::Label;
use effectcraft_effects::Buf;
use effectcraft_export::render_queue::{OutputFormat, OutputModule, RenderSettings, TimeSpan};
use effectcraft_export::tuning::{Overrides, Request, set_request};
use effectcraft_export::{Job, export};
use effectcraft_project::{Comp, ItemId, ItemKind, LayerSource, Project, Solid, build};
use effectcraft_raster::Image;
use effectcraft_render::{Accelerator, FxStep, NoFootage, Renderer};
use effectcraft_time::{FrameRate, Tick};

/// A "GPU" that renders with the CPU compositor (`decline`: refuses every frame).
struct Mock {
    decline: bool,
    asked: AtomicU64,
}

impl Accelerator for Mock {
    fn name(&self) -> String {
        "mock".into()
    }
    fn comp_frame(&self, r: &Renderer, comp: ItemId, t: Tick) -> Option<Image> {
        self.asked.fetch_add(1, Ordering::Relaxed);
        std::thread::sleep(std::time::Duration::from_millis(t.0.unsigned_abs() % 7));
        (!self.decline).then(|| r.comp_frame_cpu(comp, t))
    }
    fn supports_effect(&self, _: &str) -> bool {
        false
    }
    fn effects(&self, _: &[FxStep], _: &Buf, _: Option<f32>) -> Option<Buf> {
        None
    }
}

/// 40 frames; a solid of a different colour starts at each of several frames, so frames out of
/// order change the movie.
fn project() -> (Project, ItemId) {
    project_with(None)
}

/// [`project`], with a sound layer playing `audio`.
fn project_with(audio: Option<&Path>) -> (Project, ItemId) {
    let mut p = Project::default();
    let mut comp = Comp::new(64, 48, FrameRate::new(10, 1), Tick::from_seconds_f64(4.0));
    for (i, start) in [3, 7, 11, 17, 23, 31].into_iter().enumerate() {
        let c = [(i % 3 == 0) as u8 as f32, (i % 2) as f32, i as f32 / 6.0];
        let sid = p.add_item(&format!("S{i}"), Label::Red, None, ItemKind::Solid(Solid { color: c, width: 10 + 8 * i as u32, height: 48, pixel_aspect: 1.0 }));
        let mut l = build::layer(&mut p, &comp, &format!("S{i}"), LayerSource::Solid { item: sid }, (10 + 8 * i as u32, 48), None);
        l.in_point = comp.frame_rate.tick_of(start);
        comp.layers.push(l);
    }
    if let Some(path) = audio {
        let f = effectcraft_media::probe(path).expect("probe wav");
        let fid = p.add_item("tone.wav", Label::SeaFoam, None, ItemKind::Footage(f));
        let l = build::layer(&mut p, &comp, "tone", LayerSource::Footage { item: fid }, (0, 0), None);
        comp.layers.push(l);
    }
    let cid = p.add_item("Comp", Label::Sandstone, None, ItemKind::Comp(comp.into()));
    (p, cid)
}

fn render(p: &Project, cid: ItemId, accel: Option<&dyn Accelerator>, name: &str) -> Vec<u8> {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-out/export/scheduler");
    std::fs::create_dir_all(&d).expect("mkdir");
    let path = d.join(name).to_string_lossy().to_string();
    let s = RenderSettings { time_span: TimeSpan::LengthOfComp, ..Default::default() };
    let om = OutputModule { bitrate_kbps: 4000, ..OutputModule::for_format(OutputFormat::H264) };
    let job = Job {
        project: p,
        footage: &NoFootage,
        expr: None,
        accel,
        comp: cid,
        settings: &s,
        output: &om,
        path: &path,
        sink: None,
        options: Default::default(),
        nested_switches: true,
    };
    let mut last = 0;
    let r = export(&job, &mut |pr| {
        assert!(pr.done >= last);
        last = pr.done;
        true
    })
    .expect("export");
    assert_eq!((r.frames, last), (40, 40));
    std::fs::read(&path).expect("read")
}

#[test]
fn streams_in_order_with_any_worker_mix() {
    let (p, cid) = project();
    let reference = render(&p, cid, None, "cpu.mp4");
    for (k, (decline, cpu)) in [(false, Some(0)), (false, Some(2)), (true, Some(1)), (false, None)].into_iter().enumerate() {
        let mut cli = Overrides { cpu_frames: cpu, ..Default::default() };
        // The mock adapter is no verified vendor: opt in to the scheduler.
        cli.set("gpu-frames", "1").unwrap();
        set_request(Request { cli, ..Default::default() });
        let gpu = Mock { decline, asked: AtomicU64::new(0) };
        let movie = render(&p, cid, Some(&gpu), &format!("sched{k}.mp4"));
        assert!(movie == reference, "decline {decline} cpu {cpu:?}: differs from the CPU export");
        assert!(gpu.asked.load(Ordering::Relaxed) > 0, "the GPU worker rendered nothing");
    }
}

/// A stereo 440 Hz tone, 16-bit PCM at 48 kHz.
fn write_tone(path: &Path, secs: f64) {
    let sr = 48_000u32;
    let mut data = vec![];
    for i in 0..(secs * sr as f64) as usize {
        let s = (0.5 * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / sr as f64).sin() * 32767.0).round() as i16;
        data.extend_from_slice(&s.to_le_bytes());
        data.extend_from_slice(&s.to_le_bytes());
    }
    let mut w = b"RIFF".to_vec();
    w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    for v in [16u32, 1 | (2 << 16), sr, sr * 4, 4 | (16 << 16)] {
        w.extend_from_slice(&v.to_le_bytes());
    }
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(data.len() as u32).to_le_bytes());
    w.extend_from_slice(&data);
    std::fs::write(path, w).expect("write wav");
}

/// Every file `fmt` wrote for `path` (a movie, or each frame of an image sequence), in order.
fn written(path: &Path) -> Vec<Vec<u8>> {
    let dir = path.parent().expect("dir");
    let stem = path.file_name().expect("name").to_string_lossy().split('[').next().unwrap_or_default().to_string();
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .expect("read dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&stem)))
        .collect();
    files.sort();
    files.iter().map(|p| std::fs::read(p).expect("read")).collect()
}

/// Decoded stereo samples of `path`'s audio track (empty when it has none).
fn audio_of(path: &Path) -> Vec<f32> {
    let f = effectcraft_media::probe(path).expect("probe");
    if !f.has_audio {
        return vec![];
    }
    effectcraft_media::MediaPool::new().audio_samples(&f, Tick::ZERO, 4 * 48_000, 48_000)
}

/// Decoded frames of `path`'s 40-frame, 10 fps video track.
fn video_of(path: &Path) -> Vec<effectcraft_raster::Image> {
    let f = effectcraft_media::probe(path).expect("probe");
    let pool = effectcraft_media::MediaPool::new();
    (0..40).map(|k| (*pool.frame_at(&f, Tick::from_seconds_f64((k as f64 + 0.5) / 10.0)).expect("decode")).clone()).collect()
}

/// The scheduler changes how frames are rendered, not what the output holds. Without audio, a
/// scheduled export (frames rendered by the "GPU" worker, or declined to the CPU) writes the same
/// bytes as the CPU export in every container and image format, except WebM, whose clusters
/// follow the render batches, so its decoded frames are compared instead. With audio, the scheduled export
/// interleaves audio after every frame instead of after every batch, so the container bytes
/// differ, but the video is the same and the decoded audio is sample-identical.
#[test]
fn other_formats_and_audio_match_the_cpu_export() {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-out/export/scheduler-formats");
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("mkdir");
    let wav = d.join("tone.wav");
    write_tone(&wav, 4.0);
    let (p, cid) = project_with(Some(&wav));
    let pool = effectcraft_media::MediaPool::new();
    let mut cli = Overrides::default();
    cli.set("gpu-frames", "1").unwrap();
    set_request(Request { cli, ..Default::default() });
    let s = RenderSettings { time_span: TimeSpan::LengthOfComp, ..Default::default() };
    use effectcraft_export::render_queue::AudioOutput;
    for (fmt, name, audio) in [
        (OutputFormat::H264, "a.mp4", AudioOutput::Off),
        (OutputFormat::H264, "b.mp4", AudioOutput::Auto),
        (OutputFormat::ProRes, "a.mov", AudioOutput::Off),
        (OutputFormat::ProRes, "b.mov", AudioOutput::Auto),
        (OutputFormat::WebM, "a.webm", AudioOutput::Off),
        (OutputFormat::Gif, "a.gif", AudioOutput::Auto),
        (OutputFormat::PngSequence, "f_[#####].png", AudioOutput::Auto),
        (OutputFormat::ExrSequence, "f_[#####].exr", AudioOutput::Auto),
    ] {
        let om = OutputModule { audio, ..OutputModule::for_format(fmt) };
        let mut outs = vec![];
        for (k, accel) in
            [None, Some(Mock { decline: false, asked: AtomicU64::new(0) }), Some(Mock { decline: true, asked: AtomicU64::new(0) })].iter().enumerate()
        {
            let sub = d.join(format!("{k}"));
            std::fs::create_dir_all(&sub).expect("mkdir");
            let path = sub.join(name).to_string_lossy().to_string();
            let job = Job {
                project: &p,
                footage: &pool,
                expr: None,
                accel: accel.as_ref().map(|a| a as &dyn Accelerator),
                comp: cid,
                settings: &s,
                output: &om,
                path: &path,
                sink: None,
                options: Default::default(),
                nested_switches: true,
            };
            let r = export(&job, &mut |_| true).expect("export");
            if let Some(a) = accel {
                assert!(a.asked.load(Ordering::Relaxed) > 0, "{name}: the GPU worker rendered nothing");
            }
            outs.push((r.audio, written(Path::new(&path)), path));
        }
        assert!(!outs[0].1.is_empty(), "{name}: nothing written");
        for o in &outs[1..] {
            assert_eq!(o.0, outs[0].0, "{name}: audio on/off");
            if outs[0].0 {
                let want = audio_of(Path::new(&outs[0].2));
                assert!(!want.is_empty(), "{name}: no audio decoded");
                assert!(audio_of(Path::new(&o.2)) == want, "{name}: decoded audio differs from the CPU export");
            } else if fmt == OutputFormat::WebM {
                // Clusters follow the render batches, which differ with the scheduler and the
                // thread count; the VP9 frames are the same.
                assert!(video_of(Path::new(&o.2)) == video_of(Path::new(&outs[0].2)), "{name}: decoded video differs from the CPU export");
            } else {
                assert!(o.1 == outs[0].1, "{name}: differs from the CPU export");
            }
        }
    }
}

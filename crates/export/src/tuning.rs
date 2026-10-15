//! Export tuning: every pipeline improvement as an independent option, resolved once per
//! export from the command line, the environment, a config file and the hardware.
//!
//! Precedence per option: **CLI > environment > config file > auto**. Every option has the
//! same three spellings (see `TUNING.md`):
//!
//! | CLI                | environment                          | config key        |
//! |--------------------|--------------------------------------|-------------------|
//! | `--gpu-inflight 3` | `EFFECTCRAFT_EXPORT_GPU_INFLIGHT=3`  | `gpu-inflight = 3`|
//!
//! The config file is `--tuning-config FILE` or `EFFECTCRAFT_EXPORT_CONFIG=FILE` (`key = value`
//! lines, `#` comments). The caller (the CLI) reads the environment, the config file and the
//! host facts and passes them in a [`Request`]; this crate probes and reads nothing itself. `auto` (or no value anywhere) lets [`resolve`] decide from the
//! [`Hardware`]: GPU presence, adapter limits, VRAM (or a conservative guess), core count;
//! [`ExportTuning::explain`] says what it chose and why (`--print-tuning`).
//!
//! Without a GPU attached (or with `backend = cpu`) the export is the stock CPU path.

use std::fmt::Write as _;
use std::sync::Mutex;

use effectcraft_render::Backend;

/// Values given for the options (one layer: CLI, environment or config). `None` = not given
/// (or `auto`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overrides {
    pub backend: Option<Backend>,
    pub gpu_frames: Option<usize>,
    pub gpu_inflight: Option<usize>,
    pub cpu_frames: Option<usize>,
    pub cpu_pool: Option<usize>,
    pub gpu_pool: Option<usize>,
    pub cache_mb: Option<usize>,
    pub precomp_resident: Option<bool>,
    pub gpu_yuv: Option<bool>,
    pub async_readback: Option<bool>,
    pub vram_budget_mb: Option<usize>,
    pub ram_budget_mb: Option<usize>,
    pub cpu_batch: Option<usize>,
    pub upload_budget_mb: Option<usize>,
    pub pool_budget_mb: Option<usize>,
}

/// Every option: (key, help). The CLI flag is `--key`, the environment variable
/// `EFFECTCRAFT_EXPORT_KEY` (upper case, `-` → `_`), the config key `key`.
pub const OPTIONS: &[(&str, &str)] = &[
    ("backend", "auto|gpu|cpu: compositor for exported frames"),
    ("gpu-frames", "N: GPU worker threads recording frames (0 = the stock parallel batches)"),
    ("gpu-inflight", "N: frames per GPU worker submitted and not read back yet"),
    ("cpu-frames", "N|auto: CPU compositor workers next to the GPU (auto: adaptive by throughput)"),
    ("cpu-pool", "N: threads shared by the CPU workers' intra-frame work (0 = the whole pool)"),
    ("gpu-pool", "N: a rayon pool of its own for the GPU workers (0 = the global pool)"),
    ("cache-mb", "MB: export layer cache (static layers render and upload once; 0 = off)"),
    ("precomp-resident", "on|off: render precomp layers' nested comps resident on the GPU (not on the CPU + upload)"),
    ("gpu-yuv", "on|off: convert H.264 frames to 4:2:0 on the GPU (12 MB readback at 4K)"),
    ("async-readback", "on|off: read GPU frames back asynchronously (record the next meanwhile)"),
    ("vram-budget-mb", "MB: VRAM to plan with (default: detected; unified memory: from RAM; else a conservative 2048)"),
    ("cpu-batch", "N: CPU-compositor frames rendered at once (default: one per thread, capped by the RAM budget)"),
    ("ram-budget-mb", "MB: system RAM the export may use (default: detected); unified-memory GPUs plan their budget from it"),
    ("upload-budget-mb", "MB: uploaded layer textures kept on the GPU"),
    ("pool-budget-mb", "MB: free working textures kept on the GPU"),
];

fn parse_bool(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "1" | "on" | "true" | "yes" => Some(true),
        "0" | "off" | "false" | "no" => Some(false),
        _ => None,
    }
}

impl Overrides {
    /// Set option `key` from text; `auto` clears it. `Err` says what was expected.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let key = key.trim().trim_start_matches("--").replace('_', "-").to_ascii_lowercase();
        let v = value.trim();
        let auto = v.eq_ignore_ascii_case("auto") || v.is_empty();
        let num = || v.parse::<usize>().map_err(|_| format!("{key} {v}: expected a number or auto"));
        let flag = || parse_bool(v).ok_or_else(|| format!("{key} {v}: expected on or off"));
        macro_rules! put {
            ($f:ident, $e:expr) => {
                self.$f = if auto { None } else { Some($e?) }
            };
        }
        match key.as_str() {
            "backend" => put!(backend, Backend::parse(v).ok_or_else(|| format!("backend {v}: expected auto, gpu or cpu"))),
            "gpu-frames" => put!(gpu_frames, num()),
            "gpu-inflight" => put!(gpu_inflight, num().map(|n| n.max(1))),
            "cpu-frames" => put!(cpu_frames, num()),
            "cpu-pool" => put!(cpu_pool, num()),
            "gpu-pool" => put!(gpu_pool, num()),
            "cache-mb" => put!(cache_mb, num()),
            "precomp-resident" => put!(precomp_resident, flag()),
            "gpu-yuv" => put!(gpu_yuv, flag()),
            "async-readback" => put!(async_readback, flag()),
            "vram-budget-mb" => put!(vram_budget_mb, num()),
            "ram-budget-mb" => put!(ram_budget_mb, num()),
            "cpu-batch" => put!(cpu_batch, num().map(|n| n.max(1))),
            "upload-budget-mb" => put!(upload_budget_mb, num()),
            "pool-budget-mb" => put!(pool_budget_mb, num()),
            _ => return Err(format!("unknown export tuning option {key}")),
        }
        Ok(())
    }

    /// The `EFFECTCRAFT_EXPORT_*` variables as `get` returns them (the CLI passes the process
    /// environment; invalid values are logged and ignored).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Overrides {
        let mut o = Overrides::default();
        for (key, _) in OPTIONS {
            if let Some(v) = get(&env_name(key))
                && let Err(e) = o.set(key, &v)
            {
                log::warn!(target: "effectcraft_export", "{}: {e} (ignored)", env_name(key));
            }
        }
        o
    }

    /// `key = value` lines (`#` comments).
    pub fn from_config(text: &str) -> Result<Overrides, String> {
        let mut o = Overrides::default();
        for (n, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let (k, v) = line.split_once('=').or_else(|| line.split_once(':')).ok_or_else(|| format!("line {}: expected key = value", n + 1))?;
            o.set(k, v).map_err(|e| format!("line {}: {e}", n + 1))?;
        }
        Ok(o)
    }
}

/// `EFFECTCRAFT_EXPORT_GPU_INFLIGHT` for `gpu-inflight`.
pub fn env_name(key: &str) -> String {
    format!("EFFECTCRAFT_EXPORT_{}", key.replace('-', "_").to_ascii_uppercase())
}

/// What the export is set up from by the caller (the CLI): its options, a config file, and
/// whether to only print the tuning.
#[derive(Clone, Debug, Default)]
pub struct Request {
    pub cli: Overrides,
    /// The `EFFECTCRAFT_EXPORT_*` variables ([`Overrides::from_lookup`]).
    pub env: Overrides,
    pub config: Option<Overrides>,
    /// What the caller measured of the host; the library probes nothing itself.
    pub host: Host,
    /// `--print-tuning`: resolve, print, render nothing.
    pub print_only: bool,
}

static REQUEST: Mutex<Option<Request>> = Mutex::new(None);

/// Use `r` for the exports that follow.
pub fn set_request(r: Request) {
    *REQUEST.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
}

pub(crate) fn request() -> Request {
    REQUEST.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

/// Host facts the caller measured (the CLI: `/proc/meminfo`, `sysctl`/`vm_stat`, CIM,
/// `nvidia-smi`, sysfs). `None` = unknown: the tuning then plans conservatively.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Host {
    /// Physical memory and memory available now (free + reclaimable).
    pub ram_bytes: Option<u64>,
    pub avail_ram_bytes: Option<u64>,
    /// Performance / efficiency logical cores (Apple silicon, hybrid CPUs).
    pub core_kinds: Option<(usize, usize)>,
    /// Dedicated VRAM of the largest NVIDIA GPU (`nvidia-smi`), used for an NVIDIA adapter.
    pub nvidia_vram_bytes: Option<u64>,
    /// Dedicated VRAM another way (Linux sysfs, amdgpu), used for other adapters.
    pub other_vram_bytes: Option<u64>,
}

/// The machine an export plans for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hardware {
    /// The attached GPU; `None` = CPU only.
    pub gpu: Option<GpuHw>,
    /// Rayon threads.
    pub threads: usize,
    /// Physical memory and memory available now (free + reclaimable), when the platform says.
    pub ram_bytes: Option<u64>,
    pub avail_ram_bytes: Option<u64>,
    /// Performance / efficiency cores (Apple silicon, hybrid CPUs), when the platform says.
    pub core_kinds: Option<(usize, usize)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuHw {
    pub name: String,
    pub vendor: u32,
    pub device_type: String,
    /// Dedicated VRAM when the platform reports it.
    pub vram_bytes: Option<u64>,
    /// The GPU shares system memory (Apple silicon, integrated GPUs): its budget comes from RAM.
    pub unified: bool,
    pub max_texture_dim: u32,
    /// Readbacks complete asynchronously (false: a browser worker's deferred readbacks).
    pub native: bool,
}

impl GpuHw {
    /// GPUs the tuned GPU pipeline's auto settings were verified on: NVIDIA discrete (RTX 3090)
    /// and Apple silicon (M4). Others keep the stock path unless the user opts in.
    pub fn verified(&self) -> bool {
        (self.vendor == 0x10de && self.device_type == "DiscreteGpu") || (self.unified && self.name.starts_with("Apple"))
    }
}

/// Where a value came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Cli,
    Env,
    Config,
    Auto,
}

/// A resolved option: value, origin, reason.
#[derive(Clone, Debug, PartialEq)]
pub struct Setting<T> {
    pub value: T,
    pub source: Source,
    pub why: String,
}

/// How a job renders its frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    /// Every frame of a batch in parallel on the rayon pool, with this backend (the stock export).
    Parallel(Backend),
    /// `gpu` GPU workers and CPU workers taking frames one at a time from a shared queue.
    Scheduled { gpu: usize, cpu: CpuWorkers },
}

/// CPU compositor workers next to the GPU workers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuWorkers {
    Fixed(usize),
    /// Start with none and add them while throughput improves, up to `max`.
    Adaptive {
        max: usize,
    },
}

impl CpuWorkers {
    /// Workers to spawn.
    pub fn max(self) -> usize {
        match self {
            CpuWorkers::Fixed(n) | CpuWorkers::Adaptive { max: n } => n,
        }
    }
}

/// Every option resolved for one export ([`resolve`]).
#[derive(Clone, Debug, PartialEq)]
pub struct ExportTuning {
    pub backend: Setting<Backend>,
    pub gpu_frames: Setting<usize>,
    pub gpu_inflight: Setting<usize>,
    pub cpu_frames: Setting<CpuWorkers>,
    pub cpu_pool: Setting<usize>,
    pub gpu_pool: Setting<usize>,
    pub cache_mb: Setting<usize>,
    pub cpu_batch: Setting<usize>,
    pub precomp_resident: Setting<bool>,
    pub gpu_yuv: Setting<bool>,
    pub async_readback: Setting<bool>,
    pub vram_budget_mb: Setting<usize>,
    pub ram_budget_mb: Setting<usize>,
    pub upload_budget_mb: Setting<usize>,
    pub pool_budget_mb: Setting<usize>,
    /// Hardware it was resolved for and the frame size, for [`ExportTuning::explain`].
    pub hardware: Hardware,
    pub frame: (u32, u32),
    /// Warnings worth a `warn` log (low VRAM, a forced GPU that may not fit...).
    pub warnings: Vec<String>,
}

/// VRAM assumed when the platform does not report it.
pub const UNKNOWN_VRAM_MB: usize = 2048;
/// Unified memory: the share of RAM the GPU side may plan with, and of what is available now.
pub const UNIFIED_RAM_SHARE: (u64, u64) = (40, 60);
/// Peak working VRAM of one GPU frame, in bytes per output pixel, calibrated from measured
/// `nvidia-smi` peaks of the standalone scheduler (no working-texture reuse: every layer and
/// effect allocates its own f32 textures) on the 3 s Bench4K test comp:
///
/// * RTX 3090, same build, budgets and 3 frames in flight: 17,609 MiB at 3840×2160 and
///   9,709 MiB at 1920×1080. The fixed part (caches, pools, pipelines) cancels in the
///   difference: 7,900 MiB / 6.22 Mpx ≈ 1,332 B per pixel (≈ 83 RGBA f32 textures).
/// * GTX 1070 8 GB at 1080p: 4,948 MiB with 2 frames in flight and 4,982 MiB with 3, so a
///   frame in flight beyond the first adds little (its canvas and 4:2:0 readback, see
///   [`EXTRA_FRAME_TEXTURES`]); 4,948 MiB − 2,633 MiB for the frame ≈ 2,315 MiB fixed
///   = the 1,536 MB upload cache + the 409 MB pool + ≈ [`GPU_BASE_MB`].
/// * GTX 1070 at 4K: one frame needs ≈ 10.3 GB + the fixed part, over its 8 GB: every frame
///   ran out of memory (measured), so auto picks the CPU there.
///
/// The GPU composites in RGBA f32 whatever the project's bit depth, so the estimate depends
/// on the frame size only. #688's texture reuse and resident effects cut it severalfold.
pub const GPU_FRAME_BYTES_PER_PIXEL: u64 = 1332;
/// What each frame in flight beyond the first adds, in RGBA f32 frames (its canvas and
/// readback).
pub const EXTRA_FRAME_TEXTURES: u64 = 2;
/// Device memory besides the frames and the upload/pool budgets (pipelines, staging ring,
/// driver), measured ≈ 370 MB on the GTX 1070.
pub const GPU_BASE_MB: u64 = 384;

/// Estimated peak working VRAM, in MB, of the first GPU frame of `frame` pixels.
pub fn frame_vram_mb(frame: (u32, u32)) -> u64 {
    (frame.0 as u64 * frame.1 as u64 * GPU_FRAME_BYTES_PER_PIXEL).div_ceil(1 << 20).max(1)
}

/// Frames in flight that fit `avail_mb` of VRAM: 0 when even one does not.
pub fn frames_that_fit(frame: (u32, u32), avail_mb: u64) -> u64 {
    let first = frame_vram_mb(frame);
    let extra = (frame.0 as u64 * frame.1 as u64 * 16 * EXTRA_FRAME_TEXTURES).div_ceil(1 << 20).max(1);
    if avail_mb < first { 0 } else { 1 + (avail_mb - first) / extra }
}
/// Most frames in flight per GPU worker worth having (measured: 3 hides readback + recording).
pub const MAX_INFLIGHT: usize = 3;

fn set<T>(value: T, source: Source, why: &str) -> Setting<T> {
    Setting { value, source, why: why.to_string() }
}

/// Resolve every option: `layers` in precedence order (CLI, env, config), then the auto rules.
/// The stock export layer cache (`LayerCache::default()`): 1 GiB.
pub const STOCK_CACHE_MB: usize = 1024;

pub fn resolve(hw: &Hardware, frame: (u32, u32), layers: [(&Overrides, Source); 3]) -> ExportTuning {
    macro_rules! given {
        ($f:ident) => {
            layers.iter().find_map(|(o, s)| o.$f.clone().map(|v| (v, *s)))
        };
    }
    let mut warnings = vec![];
    let threads = hw.threads.max(1);
    let fb = frame.0 as u64 * frame.1 as u64 * 16;
    let user = |s: Source| match s {
        Source::Cli => "command line",
        Source::Env => "environment",
        Source::Config => "config file",
        Source::Auto => "auto",
    };

    // VRAM budget and the GPU-side byte budgets.
    let detected = hw.gpu.as_ref().and_then(|g| g.vram_bytes).map(|b| (b >> 20) as usize);
    let unified = hw.gpu.as_ref().is_some_and(|g| g.unified);
    let ram = match given!(ram_budget_mb) {
        Some((v, s)) => set(v, s, &format!("set on the {}", user(s))),
        None => match (hw.ram_bytes, hw.avail_ram_bytes) {
            (Some(t), Some(a)) => set((t >> 20) as usize, Source::Auto, &format!("detected ({} MB available now)", a >> 20)),
            (Some(t), None) => set((t >> 20) as usize, Source::Auto, "detected"),
            _ => set(0, Source::Auto, "not reported (no RAM-based limits)"),
        },
    };
    // Available now, capped by a user RAM budget.
    let avail_ram_mb = match (hw.avail_ram_bytes, ram.source) {
        (Some(a), Source::Auto) => (a >> 20) as usize,
        (Some(a), _) => ((a >> 20) as usize).min(ram.value),
        (None, _) => ram.value,
    };
    let vram = match given!(vram_budget_mb) {
        Some((v, s)) => set(v, s, &format!("set on the {}", user(s))),
        None => match (&hw.gpu, detected) {
            (None, _) => set(0, Source::Auto, "no GPU"),
            (Some(_), _) if unified && ram.value > 0 => {
                let (share, now) = UNIFIED_RAM_SHARE;
                let mb = (ram.value as u64 * share / 100).min(avail_ram_mb as u64 * now / 100) as usize;
                set(mb, Source::Auto, &format!("unified memory: min({share}% of {} MB RAM, {now}% of {avail_ram_mb} MB available now)", ram.value))
            }
            (Some(_), Some(mb)) => set(mb, Source::Auto, "detected dedicated VRAM"),
            (Some(_), None) => set(UNKNOWN_VRAM_MB, Source::Auto, "VRAM not reported by the platform: conservative guess"),
        },
    };
    let vb = vram.value as u64;
    let upload = match given!(upload_budget_mb) {
        Some((v, s)) => set(v, s, &format!("set on the {}", user(s))),
        None if unified => {
            // What the frame slots leave of the budget (an upload is a copy within shared
            // memory, so the cache only needs to hold the static layers, not more).
            let per_frame = frame_vram_mb(frame);
            let avail0 = (vb * 9 / 10).saturating_sub(256);
            let fit = frames_that_fit(frame, avail0).min(MAX_INFLIGHT as u64);
            let used = if fit == 0 { 0 } else { per_frame + (fit - 1) * (fb * EXTRA_FRAME_TEXTURES).div_ceil(1 << 20) };
            let left = if fit == 0 { 0 } else { avail0.saturating_sub(used) };
            set(
                (256 + left).clamp(256, 1024) as usize,
                Source::Auto,
                &format!("unified memory: 256 MB + what {fit} frame slot(s) leave of the budget, at most 1024 MB (an upload is a copy within shared memory)"),
            )
        }
        None => set(
            (vb / 4).clamp(256, 1536) as usize,
            Source::Auto,
            "25% of the VRAM budget, 256..1536 MB: static layers upload once (a smaller cache re-uploads them every frame)",
        ),
    };
    let pool = match given!(pool_budget_mb) {
        Some((v, s)) => set(v, s, &format!("set on the {}", user(s))),
        None => set((vb / 20).clamp(128, 768) as usize, Source::Auto, "5% of the VRAM budget, 128..768 MB"),
    };
    // Frames that fit: 90% of VRAM minus the upload cache, the texture pool and the fixed
    // part, over the calibrated working set of a frame ([`GPU_FRAME_BYTES_PER_PIXEL`]).
    let per_frame_mb = frame_vram_mb(frame);
    let avail_mb = (vb * 9 / 10).saturating_sub(upload.value as u64 + pool.value as u64 + GPU_BASE_MB);
    let slots = frames_that_fit(frame, avail_mb) as usize;
    let gpu_ok = hw.gpu.is_some();
    let verified = hw.gpu.as_ref().is_some_and(GpuHw::verified);

    let backend = match given!(backend) {
        Some((b, s)) => {
            if b == Backend::Gpu && gpu_ok && slots == 0 {
                warnings.push(format!(
                    "backend gpu forced, but one {}x{} frame needs ~{per_frame_mb} MB of working VRAM and the budget leaves {avail_mb} MB: frames the GPU declines render on the CPU (each logged with the reason)",
                    frame.0, frame.1
                ));
            }
            set(b, s, &format!("set on the {}", user(s)))
        }
        None if !gpu_ok => set(Backend::Auto, Source::Auto, "no GPU attached: stock CPU path"),
        None if !verified => {
            let g = hw.gpu.as_ref();
            set(
                Backend::Auto,
                Source::Auto,
                &format!(
                    "GPU {} (vendor 0x{:04x}, {}) is not one the tuned pipeline was verified on (NVIDIA discrete, Apple silicon): stock path; opt in with --backend gpu or individual options",
                    g.map(|g| g.name.as_str()).unwrap_or("?"),
                    g.map(|g| g.vendor).unwrap_or(0),
                    g.map(|g| g.device_type.as_str()).unwrap_or("?")
                ),
            )
        }
        None if slots == 0 && vram.why.starts_with("VRAM not reported") => {
            warnings.push(format!(
                "GPU {}: VRAM not reported, so the tuned pipeline cannot size its frames: stock path (set vram-budget-mb to opt in)",
                hw.gpu.as_ref().map(|g| g.name.as_str()).unwrap_or("?")
            ));
            set(Backend::Auto, Source::Auto, "VRAM unknown: stock path")
        }
        None if slots == 0 => {
            warnings.push(format!(
                "GPU {} has a {} MB {} budget; one {}x{} frame needs ~{per_frame_mb} MB of working memory and the budget leaves {avail_mb} MB after the {} MB upload cache, the texture pool and the device's fixed use: rendering on the CPU up front instead of failing on the GPU frame after frame (GPU memory savings such as working-texture reuse and GPU-resident effects, #688, would let it render on the GPU)",
                hw.gpu.as_ref().map(|g| g.name.as_str()).unwrap_or("?"),
                vb,
                if unified { "unified-memory" } else { "VRAM" },
                frame.0,
                frame.1,
                upload.value
            ));
            set(Backend::Cpu, Source::Auto, "a frame does not fit the VRAM budget")
        }
        None => set(Backend::Auto, Source::Auto, "GPU attached: GPU workers + adaptive CPU workers"),
    };
    // Unverified GPUs keep the stock path unless the user chose the backend.
    // VRAM not reported and too small a guess for a frame: the stock path, as for unverified GPUs.
    let vram_unknown = backend.source == Source::Auto && backend.why == "VRAM unknown: stock path";
    let use_gpu = gpu_ok && backend.value != Backend::Cpu && !vram_unknown && (verified || backend.source != Source::Auto);

    // P/E cores scaled to the threads in use (`--threads` simulates a smaller CPU).
    let efficiency = hw.core_kinds.filter(|(p, e)| *p > 0 && *e > 0).map(|(p, e)| {
        let total = p + e;
        if threads >= total {
            return (p, e);
        }
        let p2 = (p * threads).div_ceil(total).min(threads.saturating_sub(1)).max(1);
        (p2, threads.saturating_sub(p2).max(1))
    });
    let gpu_frames = match given!(gpu_frames) {
        Some((v, s)) => set(v, s, &format!("set on the {}", user(s))),
        None if !use_gpu => set(0, Source::Auto, "no GPU in use"),
        None => set(1, Source::Auto, "one recording worker: with async readback its CPU side (~15 ms/frame) is far below the GPU's; a second only adds VRAM"),
    };
    let gpu_inflight = match given!(gpu_inflight) {
        Some((v, s)) => set(v.max(1), s, &format!("set on the {}", user(s))),
        None if !use_gpu => set(1, Source::Auto, "no GPU in use"),
        None => {
            let per_worker = slots / gpu_frames.value.max(1);
            // One frame in flight until the retry after a GPU out-of-memory is deterministic:
            // with more, a frame that runs out of memory renders differently on its retry and
            // so do a few after it (see TUNING.md). The VRAM plan below only reports the room.
            set(
                1,
                Source::Auto,
                &format!(
                    "one frame: a retry after GPU out-of-memory is not deterministic yet (VRAM budget: {avail_mb} MB free after the upload cache, pool and fixed use; ~{per_frame_mb} MB for the first {}x{} frame = room for {} per worker, at most {MAX_INFLIGHT})",
                    frame.0,
                    frame.1,
                    per_worker.clamp(1, MAX_INFLIGHT)
                ),
            )
        }
    };
    let cpu_frames = match given!(cpu_frames) {
        Some((v, s)) => set(CpuWorkers::Fixed(v), s, &format!("set on the {}", user(s))),
        None if !use_gpu => set(CpuWorkers::Fixed(0), Source::Auto, "no GPU: every frame on the CPU compositor (stock parallel batches)"),
        None if backend.value == Backend::Gpu => set(CpuWorkers::Fixed(0), Source::Auto, "backend gpu: GPU workers only"),
        None => match efficiency {
            Some((_, e)) => set(
                CpuWorkers::Adaptive { max: e.clamp(1, 8) },
                Source::Auto,
                &format!("adaptive: start GPU-only, add CPU workers while measured frames/s improves (max the {e} efficiency core(s), ≤ 8)"),
            ),
            None => set(
                CpuWorkers::Adaptive { max: (threads / 2).clamp(1, 8) },
                Source::Auto,
                "adaptive: start GPU-only, add CPU workers while measured frames/s improves (max half the threads, ≤ 8)",
            ),
        },
    };
    let cpu_pool = match given!(cpu_pool) {
        Some((v, s)) => set(v, s, &format!("set on the {}", user(s))),
        None if cpu_frames.value.max() == 0 => set(0, Source::Auto, "no CPU workers"),
        None => match efficiency {
            Some((p, e)) => set(
                e.max(1),
                Source::Auto,
                &format!("the {e} efficiency core(s): the {p} performance core(s) stay with the GPU worker's CPU side and the encoder"),
            ),
            None => set((threads / 2).max(1), Source::Auto, "half the threads: CPU workers must not starve the GPU worker's CPU side"),
        },
    };
    let gpu_pool = match given!(gpu_pool) {
        Some((v, s)) => set(v, s, &format!("set on the {}", user(s))),
        None => set(0, Source::Auto, "the global pool"),
    };
    let cache_mb = match given!(cache_mb) {
        Some((v, s)) => set(v, s, &format!("set on the {}", user(s))),
        None => set(STOCK_CACHE_MB, Source::Auto, "the job-wide layer cache at the stock 1 GiB (a larger one is opt-in)"),
    };
    // CPU-compositor frames rendered at once (stock: one per thread), capped so their working
    // memory (~8 × the frame's f32 buffer each) fits 60% of the RAM budget after the layer cache.
    let cpu_frame_mb = (fb * 8).div_ceil(1 << 20).max(1) as usize;
    let cpu_batch = match given!(cpu_batch) {
        Some((v, s)) => set(v.max(1), s, &format!("set on the {}", user(s))),
        None if ram.value == 0 => set(threads, Source::Auto, "one frame per thread (stock)"),
        None => {
            let room = (ram.value.min(avail_ram_mb.max(1)) * 6 / 10).saturating_sub(cache_mb.value);
            let fit = (room / cpu_frame_mb).max(1);
            if fit >= threads {
                set(threads, Source::Auto, &format!("one frame per thread (stock): {threads} x ~{cpu_frame_mb} MB fit 60% of the RAM budget"))
            } else {
                set(
                    fit,
                    Source::Auto,
                    &format!("{fit} frame(s) at once: ~{cpu_frame_mb} MB each must fit {room} MB (60% of the RAM budget after the layer cache)"),
                )
            }
        }
    };
    let on_gpu = |given: Option<(bool, Source)>, why: &str| match given {
        Some((v, s)) => set(v, s, &format!("set on the {}", user(s))),
        // On by default in the GPU compositor either way; the export only turns it off when asked.
        None if !use_gpu => set(true, Source::Auto, "the GPU compositor's default"),
        None => set(true, Source::Auto, why),
    };
    let precomp_resident = on_gpu(given!(precomp_resident), "nested comps render on the GPU (no CPU render + upload per frame)");
    let native = hw.gpu.as_ref().is_some_and(|g| g.native);
    let gpu_yuv = match given!(gpu_yuv) {
        Some((v, s)) => set(v, s, &format!("set on the {}", user(s))),
        None if !use_gpu || !native => set(false, Source::Auto, "no native GPU in use"),
        None => set(true, Source::Auto, "H.264: 12 MB 4:2:0 readback instead of 133 MB RGBA f32 at 4K (byte-identical conversion)"),
    };
    let async_readback = match given!(async_readback) {
        Some((v, s)) => set(v, s, &format!("set on the {}", user(s))),
        None if !gpu_yuv.value => set(false, Source::Auto, "only with gpu-yuv"),
        None => set(true, Source::Auto, "the GPU worker records the next frame while the copy runs"),
    };
    if let Some(g) = &hw.gpu
        && use_gpu
        && (frame.0 > g.max_texture_dim || frame.1 > g.max_texture_dim)
    {
        warnings.push(format!("{}x{} exceeds the GPU's {} px texture limit: frames render on the CPU (logged)", frame.0, frame.1, g.max_texture_dim));
    }
    ExportTuning {
        backend,
        gpu_frames,
        gpu_inflight,
        cpu_frames,
        cpu_pool,
        gpu_pool,
        cache_mb,
        cpu_batch,
        precomp_resident,
        gpu_yuv,
        async_readback,
        vram_budget_mb: vram,
        ram_budget_mb: ram,
        upload_budget_mb: upload,
        pool_budget_mb: pool,
        hardware: hw.clone(),
        frame,
        warnings,
    }
}

impl ExportTuning {
    /// The scheduling plan.
    pub(crate) fn plan(&self, accel: bool) -> Plan {
        let backend = self.backend.value;
        let gpu = self.gpu_frames.value;
        if !accel || backend == Backend::Cpu || gpu == 0 {
            return Plan::Parallel(backend);
        }
        Plan::Scheduled { gpu, cpu: self.cpu_frames.value }
    }

    /// The flags the GPU compositor reads.
    pub fn gpu_flags(&self) -> effectcraft_render::gpu_flags::GpuFlags {
        effectcraft_render::gpu_flags::GpuFlags {
            precomp_resident: self.precomp_resident.value,
            unified: self.hardware.gpu.as_ref().is_some_and(|g| g.unified),
        }
    }

    /// A table of every option: value, where it came from, why.
    pub fn explain(&self) -> String {
        let mut s = String::new();
        let hw = &self.hardware;
        let _ = writeln!(s, "export tuning for a {}x{} frame, {} thread(s)", self.frame.0, self.frame.1, hw.threads);
        let _ = writeln!(
            s,
            "CPU cores: {}; RAM: {}",
            hw.core_kinds.map(|(p, e)| format!("{p} performance + {e} efficiency")).unwrap_or("kinds not reported".into()),
            match (hw.ram_bytes, hw.avail_ram_bytes) {
                (Some(t), Some(a)) => format!("{} MB, {} MB available", t >> 20, a >> 20),
                (Some(t), None) => format!("{} MB", t >> 20),
                _ => "not reported".into(),
            }
        );
        match &hw.gpu {
            Some(g) => {
                let _ = writeln!(
                    s,
                    "GPU: {} (vendor 0x{:04x}, {}), VRAM {}, max texture {} px",
                    g.name,
                    g.vendor,
                    g.device_type,
                    if g.unified { "unified (shares RAM)".into() } else { g.vram_bytes.map(|b| format!("{} MB", b >> 20)).unwrap_or("not reported".into()) },
                    g.max_texture_dim
                );
            }
            None => {
                let _ = writeln!(s, "GPU: none attached");
            }
        }
        macro_rules! row {
            ($name:expr, $f:expr) => {{
                let v = &$f;
                let _ = writeln!(s, "  {:<17} {:<22} [{:?}] {}", $name, format!("{:?}", v.value), v.source, v.why);
            }};
        }
        row!("backend", self.backend);
        row!("gpu-frames", self.gpu_frames);
        row!("gpu-inflight", self.gpu_inflight);
        row!("cpu-frames", self.cpu_frames);
        row!("cpu-pool", self.cpu_pool);
        row!("gpu-pool", self.gpu_pool);
        row!("cache-mb", self.cache_mb);
        row!("cpu-batch", self.cpu_batch);
        row!("precomp-resident", self.precomp_resident);
        row!("gpu-yuv", self.gpu_yuv);
        row!("async-readback", self.async_readback);
        row!("vram-budget-mb", self.vram_budget_mb);
        row!("ram-budget-mb", self.ram_budget_mb);
        row!("upload-budget-mb", self.upload_budget_mb);
        row!("pool-budget-mb", self.pool_budget_mb);
        for w in &self.warnings {
            let _ = writeln!(s, "  warning: {w}");
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UHD: (u32, u32) = (3840, 2160);

    fn hw(vram_gb: Option<u64>, threads: usize) -> Hardware {
        Hardware {
            gpu: vram_gb.map(|g| GpuHw {
                name: "Test GPU".into(),
                vendor: 0x10de,
                device_type: "DiscreteGpu".into(),
                vram_bytes: if g == 0 { None } else { Some(g << 30) },
                unified: false,
                max_texture_dim: 16384,
                native: true,
            }),
            threads,
            ..Default::default()
        }
    }

    /// A verified GPU whose VRAM the platform does not report keeps the stock path at 4K
    /// (never the CPU backend), and a user VRAM budget opts it back in.
    #[test]
    fn unknown_vram_keeps_the_stock_gpu_path() {
        let t = auto(&hw(Some(0), 16), UHD);
        assert_eq!(t.backend.value, Backend::Auto);
        assert!(matches!(t.plan(true), Plan::Parallel(Backend::Auto)));
        let cli = Overrides { vram_budget_mb: Some(24576), ..Default::default() };
        let t = resolve(&hw(Some(0), 16), UHD, [(&cli, Source::Cli), (&Overrides::default(), Source::Env), (&Overrides::default(), Source::Config)]);
        assert!(matches!(t.plan(true), Plan::Scheduled { .. }));
    }

    /// Apple M4-like: 16 GB unified memory, `avail_gb` available, 4P + 6E cores.
    fn m4(avail_gb: u64, threads: usize) -> Hardware {
        Hardware {
            gpu: Some(GpuHw {
                name: "Apple M4".into(),
                device_type: "IntegratedGpu".into(),
                unified: true,
                max_texture_dim: 16384,
                native: true,
                ..Default::default()
            }),
            threads,
            ram_bytes: Some(16 << 30),
            avail_ram_bytes: Some(avail_gb << 30),
            core_kinds: Some((4, 6)),
        }
    }

    fn vendor(vendor: u32, device_type: &str, name: &str, unified: bool) -> Hardware {
        let mut h = hw(Some(16), 16);
        if let Some(g) = &mut h.gpu {
            (g.vendor, g.device_type, g.name, g.unified) = (vendor, device_type.into(), name.into(), unified);
        }
        h
    }

    #[test]
    fn auto_tunes_only_verified_gpus() {
        // Verified: NVIDIA discrete, Apple silicon.
        assert!(matches!(auto(&hw(Some(24), 16), UHD).plan(true), Plan::Scheduled { .. }));
        assert!(matches!(auto(&m4(11, 10), (1920, 1080)).plan(true), Plan::Scheduled { .. }));
        // AMD, Intel, NVIDIA integrated, unknown: the stock path, saying why.
        for h in [
            vendor(0x1002, "DiscreteGpu", "AMD Radeon RX 7900", false),
            vendor(0x8086, "IntegratedGpu", "Intel Arc", true),
            vendor(0x10de, "IntegratedGpu", "NVIDIA Tegra", true),
            vendor(0x1234, "Other", "Mystery GPU", false),
        ] {
            let t = auto(&h, UHD);
            assert_eq!(t.plan(true), Plan::Parallel(Backend::Auto), "{:?}", h.gpu);
            assert!(t.backend.why.contains("not one the tuned pipeline was verified on"));
            assert!(!t.gpu_yuv.value && !t.async_readback.value);
            // Opting in: the backend, or an individual option.
            let mut cli = Overrides::default();
            cli.set("backend", "gpu").unwrap();
            let n = Overrides::default();
            let t = resolve(&h, UHD, [(&cli, Source::Cli), (&n, Source::Env), (&n, Source::Config)]);
            assert!(matches!(t.plan(true), Plan::Scheduled { .. }) && t.precomp_resident.value);
            let mut cli = Overrides::default();
            cli.set("precomp-resident", "on").unwrap();
            let t = resolve(&h, UHD, [(&cli, Source::Cli), (&n, Source::Env), (&n, Source::Config)]);
            assert!(t.precomp_resident.value);
        }
    }

    #[test]
    fn cpu_batch_fits_the_ram_budget() {
        // No RAM reported: stock (a frame per thread).
        assert_eq!(auto(&hw(None, 16), UHD).cpu_batch.value, 16);
        // 4 GB budget at 4K: ~1013 MB per CPU frame, 60% of 4096 - 1024 cache = 1433 MB: 1 frame.
        let mut cli = Overrides::default();
        cli.set("ram-budget-mb", "4096").unwrap();
        let n = Overrides::default();
        let t = resolve(&m4(11, 10), UHD, [(&cli, Source::Cli), (&n, Source::Env), (&n, Source::Config)]);
        assert_eq!((t.backend.value, t.cpu_batch.value), (Backend::Cpu, 1));
        assert!(t.warnings[0].contains("after the 256 MB upload cache"), "{}", t.warnings[0]);
        // Plenty of RAM at 1080p: one per thread.
        assert_eq!(auto(&m4(11, 10), (1920, 1080)).cpu_batch.value, 10);
    }

    #[test]
    fn unified_memory_plans_from_ram() {
        // 16 GB, 11 GB available: min(40% of 16384, 60% of 11264) = 6553 MB. A 4K frame needs
        // ~10.3 GB without texture reuse: the CPU, with a warning; 1080p fits on the GPU.
        let t = auto(&m4(11, 10), UHD);
        assert_eq!(t.vram_budget_mb.value, 6553);
        assert_eq!((t.backend.value, t.upload_budget_mb.value), (Backend::Cpu, 256));
        assert_eq!(t.warnings.len(), 1);
        let t = auto(&m4(11, 10), (1920, 1080));
        assert_eq!((t.backend.value, t.gpu_inflight.value, t.upload_budget_mb.value), (Backend::Auto, 1, 1024));
        assert_eq!((t.cpu_frames.value, t.cpu_pool.value), (CpuWorkers::Adaptive { max: 6 }, 6));
        assert_eq!(t.cache_mb.value, STOCK_CACHE_MB);
        // Little available now: the budget shrinks and 1080p falls back to the CPU too.
        let t = auto(&m4(3, 10), (1920, 1080));
        assert_eq!(t.vram_budget_mb.value, 1843);
        assert_eq!(t.backend.value, Backend::Cpu);
        assert_eq!(t.warnings.len(), 1);
        // 4 threads: 2P + 2E.
        let t = auto(&m4(11, 4), (1920, 1080));
        assert_eq!((t.cpu_frames.value, t.cpu_pool.value), (CpuWorkers::Adaptive { max: 2 }, 2));
        // A user RAM budget caps it.
        let mut cli = Overrides::default();
        cli.set("ram-budget-mb", "4096").unwrap();
        let n = Overrides::default();
        let t = resolve(&m4(11, 10), UHD, [(&cli, Source::Cli), (&n, Source::Env), (&n, Source::Config)]);
        assert_eq!((t.vram_budget_mb.value, t.backend.value), (1638, Backend::Cpu));
    }

    fn auto(h: &Hardware, frame: (u32, u32)) -> ExportTuning {
        let n = Overrides::default();
        resolve(h, frame, [(&n, Source::Cli), (&n, Source::Env), (&n, Source::Config)])
    }

    #[test]
    fn no_gpu_is_stock() {
        for threads in [4, 8, 16, 32] {
            let t = auto(&hw(None, threads), UHD);
            assert_eq!(t.plan(false), Plan::Parallel(Backend::Auto));
            assert_eq!(t.cache_mb.value, STOCK_CACHE_MB);
            assert!(!t.gpu_yuv.value && !t.async_readback.value);
            assert!(t.warnings.is_empty());
        }
    }

    #[test]
    fn vram_profiles_at_4k() {
        // 24 GB: room for three frames in flight, one by default.
        let t = auto(&hw(Some(24), 16), UHD);
        assert_eq!((t.backend.value, t.gpu_frames.value, t.gpu_inflight.value), (Backend::Auto, 1, 1));
        assert_eq!((t.upload_budget_mb.value, t.pool_budget_mb.value), (1536, 768));
        // 8 GB (GTX 1070): 7372 - 1536 upload - 409 pool - 384 = 5043 MB left; a 4K frame needs
        // ~10537 MB (measured: every frame ran out of memory), so the CPU up front, with one
        // warning naming the GPU memory savings that would fit it. 1080p (~2635 MB) fits.
        assert_eq!(frame_vram_mb(UHD), 10537);
        assert_eq!(frame_vram_mb((1920, 1080)), 2635);
        let t = auto(&hw(Some(8), 8), UHD);
        assert_eq!((t.backend.value, t.plan(true)), (Backend::Cpu, Plan::Parallel(Backend::Cpu)));
        assert_eq!((t.upload_budget_mb.value, t.pool_budget_mb.value), (1536, 409));
        assert_eq!(t.warnings.len(), 1);
        assert!(t.warnings[0].contains("rendering on the CPU up front") && t.warnings[0].contains("#688"), "{}", t.warnings[0]);
        let t = auto(&hw(Some(8), 8), (1920, 1080));
        assert_eq!((t.backend.value, t.gpu_inflight.value), (Backend::Auto, 1));
        assert!(t.warnings.is_empty());
        // 4 GB and 2 GB: neither size fits.
        for gb in [4, 2] {
            for frame in [UHD, (1920, 1080)] {
                let t = auto(&hw(Some(gb), 4), frame);
                assert_eq!(t.backend.value, Backend::Cpu, "{gb} GB {frame:?}");
                assert_eq!(t.warnings.len(), 1);
            }
        }
        assert_eq!(auto(&hw(Some(4), 4), UHD).upload_budget_mb.value, 1024);
        // Unknown VRAM plans with a conservative 2 GB.
        let t = auto(&hw(Some(0), 16), (1920, 1080));
        assert_eq!(t.vram_budget_mb.value, UNKNOWN_VRAM_MB);
    }

    #[test]
    fn frames_in_flight_from_the_calibrated_estimate() {
        // The 24 GB GPU has room for three 4K frames in flight but keeps one (the default);
        // one frame more than fits is never planned.
        let t = auto(&hw(Some(24), 16), UHD);
        assert_eq!((t.backend.value, t.gpu_inflight.value), (Backend::Auto, 1));
        assert_eq!(frames_that_fit(UHD, 10536), 0);
        assert_eq!(frames_that_fit(UHD, 10537), 1);
        assert_eq!(frames_that_fit(UHD, 10537 + 254), 2);
    }

    #[test]
    fn cores() {
        for (threads, max, pool) in [(4, 2, 2), (8, 4, 4), (16, 8, 8), (32, 8, 16)] {
            let t = auto(&hw(Some(24), threads), UHD);
            assert_eq!(t.cpu_frames.value, CpuWorkers::Adaptive { max }, "{threads} threads");
            assert_eq!(t.cpu_pool.value, pool);
            assert_eq!(t.plan(true), Plan::Scheduled { gpu: 1, cpu: CpuWorkers::Adaptive { max } });
        }
    }

    #[test]
    fn precedence_cli_env_config_auto() {
        let mut cli = Overrides::default();
        let mut env = Overrides::default();
        let mut cfg = Overrides::default();
        cfg.set("gpu-inflight", "1").unwrap();
        cfg.set("cache-mb", "512").unwrap();
        env.set("gpu-inflight", "2").unwrap();
        cli.set("--gpu-inflight", "3").unwrap();
        env.set("precomp_resident", "off").unwrap();
        let t = resolve(&hw(Some(4), 8), UHD, [(&cli, Source::Cli), (&env, Source::Env), (&cfg, Source::Config)]);
        assert_eq!((t.gpu_inflight.value, t.gpu_inflight.source), (3, Source::Cli));
        assert_eq!((t.precomp_resident.value, t.precomp_resident.source), (false, Source::Env));
        assert_eq!((t.cache_mb.value, t.cache_mb.source), (512, Source::Config));
        assert_eq!(t.gpu_yuv.source, Source::Auto);
    }

    #[test]
    fn forced_gpu_on_small_vram_warns() {
        let mut cli = Overrides::default();
        cli.set("backend", "gpu").unwrap();
        let n = Overrides::default();
        let t = resolve(&hw(Some(2), 4), UHD, [(&cli, Source::Cli), (&n, Source::Env), (&n, Source::Config)]);
        assert_eq!(t.backend.value, Backend::Gpu);
        assert_eq!(t.warnings.len(), 1);
        assert_eq!(t.cpu_frames.value, CpuWorkers::Fixed(0));
    }

    #[test]
    fn parsing() {
        let o = Overrides::from_lookup(|k| match k {
            "EFFECTCRAFT_EXPORT_GPU_YUV" => Some("off".into()),
            "EFFECTCRAFT_EXPORT_CPU_FRAMES" => Some("auto".into()),
            _ => None,
        });
        assert_eq!((o.gpu_yuv, o.cpu_frames), (Some(false), None));
        let c = Overrides::from_config("# x\nbackend = gpu\ngpu-frames: 2\n").unwrap();
        assert_eq!((c.backend, c.gpu_frames), (Some(Backend::Gpu), Some(2)));
        assert!(Overrides::from_config("nope = 1").is_err());
        assert!(Overrides::default().set("gpu-yuv", "maybe").is_err());
        assert_eq!(env_name("gpu-inflight"), "EFFECTCRAFT_EXPORT_GPU_INFLIGHT");
    }
}

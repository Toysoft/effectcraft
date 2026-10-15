# Export tuning

Every pipeline improvement of the GPU export is an independent option. Each has three
spellings, resolved per export with precedence **CLI > environment > config file > auto**:

| CLI (`render`)        | environment                          | config file          |
|-----------------------|--------------------------------------|----------------------|
| `--gpu-inflight 3`    | `EFFECTCRAFT_EXPORT_GPU_INFLIGHT=3`  | `gpu-inflight = 3`   |

Config file: `--tuning-config FILE` or `EFFECTCRAFT_EXPORT_CONFIG=FILE` (`key = value` lines,
`#` comments). The CLI reads the environment variables and the config file and passes them to the
export library (`tuning::Request`); other callers pass their own values or none. The value `auto` (or leaving an option out) lets the auto rules decide.
`--print-tuning` prints every resolved value, where it came from and why, and renders nothing.
`--threads N` / `EFFECTCRAFT_EXPORT_THREADS` sizes the rayon pool (simulates a smaller CPU).
`RUST_LOG=effectcraft_export=info` logs the same table at the start of every export.

## Options

| option | values | auto |
|---|---|---|
| `backend` | auto, gpu, cpu | `auto` with a GPU (GPU workers + adaptive CPU workers); stock path without one; `cpu` if one frame does not fit the VRAM budget (warning logged) |
| `gpu-frames` | N (0 = stock batches) | 1 GPU recording worker |
| `gpu-inflight` | N ≥ 1 | 1 (below: room for up to 3 is reported, not used) |
| `cpu-frames` | N, auto | adaptive: start GPU-only, add CPU workers while measured frames/s improves, max min(threads/2, 8); 0 with `backend gpu` |
| `cpu-pool` | N (0 = global pool) | threads/2 when there are CPU workers |
| `gpu-pool` | N (0 = global pool) | 0 |
| `cache-mb` | MB (0 = off) | 1024: the job-wide layer cache at the stock 1 GiB; a larger cache is opt-in |
| `precomp-resident` | on, off | on with a GPU: precomp layers' nested comps render on the GPU (2D precomp layers without masks/styles/proxies/overrides/frame blending; others take the CPU path) |
| `gpu-yuv` | on, off | on with a native GPU (H.264 only; byte-identical to the CPU conversion) |
| `async-readback` | on, off | on when `gpu-yuv` is on (off = wait for each frame) |
| `cpu-batch` | N | one CPU-compositor frame per thread, capped so ~8x the frame buffer each fits 60% of the RAM budget after the layer cache |
| `ram-budget-mb` | MB | detected RAM: unified-memory GPUs plan from it; the layer cache is capped at 15% of it |
| `vram-budget-mb` | MB | dedicated VRAM measured by the CLI (`nvidia-smi`; amdgpu sysfs on Linux); unified memory: from RAM; else 2048 |
| `upload-budget-mb` | MB | 25% of the VRAM budget, clamped to 256..1536 (a warning is logged once when it overflows) |
| `pool-budget-mb` | MB | 5% of the VRAM budget, clamped to 128..768 |

## Auto rules

* **No GPU attached** (or `backend cpu`): the stock CPU path: parallel batches, the job layer cache,
  software encoder, the project's renderer. The output is byte-identical to the stock export.
* **VRAM plan**: the first frame in flight needs about 1,332 B per output pixel (≈ 10.3 GB at 4K,
  2.6 GB at 1080p), calibrated from measured peaks without working-texture reuse (the GPU composites in
  RGBA f32 whatever the project's bit depth); each further frame adds two RGBA f32 frames.
  `free = 90% of budget − upload cache − texture pool − 384 MB`, and room for `clamp(frames that fit / gpu-frames, 1, 3)` frames in flight (`--print-tuning` shows it).
  The default `gpu-inflight` is **1** all the same: with more, a frame that runs out of GPU memory is
  retried, and the retry (plus a few frames after it) does not render exactly as it would have without
  the failure, so the same export can differ between runs (Singularity at 1080p: frames 55-59 at ~30 dB).
  Until that is fixed, set `gpu-inflight` above 1 only where a run-to-run difference is acceptable.
  One frame in flight also measured no slower there (46.8 s vs 43-65 s with 3, which kept running out of memory).
  Room at 4K: 24 GB → 3; 16 GB → 3; 8 GB and less → 0, so the backend becomes `cpu` up front and one warning says why
  (GPU memory savings such as #688's texture reuse would let it render on the GPU). 1080p: 8 GB → 3, 4 GB and less → `cpu`.
  A forced `backend gpu` that does not fit is kept but warned about.
* **Out of memory while rendering**: a failed GPU allocation (reported to the device's error handler from
  whichever thread allocated) invalidates only that frame: the device stays in use. The GPU worker waits for its frames in flight, keeps one fewer in flight from then on (at least 1, logged once)
  and retries the frame on the GPU; only a frame that fails again is rendered on the CPU.
* **CPU workers** stay adaptive by measured throughput. Before the first one is tried, a CPU frame is
  estimated (frame rendered at quarter size, ×16). No worker is tried when a CPU frame would take more than
  half the reorder buffer's worth of GPU frames (it would stall the in-order encoder) or add < 3%. Every frame the GPU declines is rendered on
  the CPU and logged as a warning with the reason (device failure / out of memory, unsupported
  content, frame larger than the texture limit), and the end summary counts these frames. So there
  is no silent fallback.
* **Encoder:** H.264 always uses the pure-Rust software encoder (no hardware encoder).

## Unified memory (Apple silicon, integrated GPUs)

* Detected from the adapter (integrated, or an Apple GPU). There is no separate VRAM: the GPU budget
  (`vram-budget-mb`) is min(40% of RAM, 60% of the RAM available when the export starts); RAM and
  availability come from `sysctl hw.memsize` + `vm_stat` (macOS) or `/proc/meminfo` (Linux), measured by
  the CLI and passed in (`tuning::Host`); the export library itself spawns no processes and reads no system files.
* The upload cache gets 256 MB plus what the frame slots leave of that budget (at most 1024 MB): an
  upload is a copy within shared memory, so frame slots come first.
* The CPU layer cache stays at the stock 1 GiB unless `cache-mb` asks for more.
* Performance/efficiency cores (`hw.perflevel0/1`): adaptive CPU workers and their pool use at most
  the efficiency cores; the performance cores stay with the GPU worker's CPU side and the encoder.
  With `--threads N` the split is scaled to N.
* If a frame does not fit the budget (little free memory, small `ram-budget-mb`), the export renders
  on the CPU and logs a warning saying so.

## Which GPUs get the tuned pipeline automatically

`auto` applies the tuned GPU pipeline (scheduler, resident precomps,
GPU YUV, budgets) only on GPUs it was verified on: **NVIDIA discrete** (RTX 3090, Windows) and
**Apple silicon** (M4, macOS). On AMD, Intel, NVIDIA integrated and unknown adapters, `auto` keeps
the stock path and `--print-tuning` / the export log say why. Users opt in with
`--backend gpu` (the whole tuned pipeline) or with individual options (e.g. `--precomp-resident on`).

## Memory limits on the CPU path

With a RAM budget (detected, or `ram-budget-mb`), the CPU compositor renders at most `cpu-batch`
frames at once, so a constrained machine (or a GPU-less fallback) does not run out of memory.
The layer cache is capped at 15% of the budget.

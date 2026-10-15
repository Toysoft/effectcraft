//! Hardware the export tuning plans for ([`crate::tuning::Hardware`]). The library probes
//! nothing itself (no process spawns, no system files): the adapter comes from the
//! accelerator and the host facts (RAM, core kinds, VRAM) from the caller ([`Host`]).

use crate::tuning::{GpuHw, Hardware, Host};

/// The machine as seen by this export: rayon threads, the attached accelerator and what the
/// caller measured of the host.
pub(crate) fn detect(accel: Option<&dyn effectcraft_render::Accelerator>, host: &Host) -> Hardware {
    let gpu = accel.map(|a| {
        let info = a.adapter_info().unwrap_or_default();
        let vram_bytes = if info.vendor == 0x10de { host.nvidia_vram_bytes.or(host.other_vram_bytes) } else { host.other_vram_bytes };
        // Apple silicon and integrated GPUs share system memory.
        let unified = info.device_type == "IntegratedGpu" || info.name.starts_with("Apple");
        GpuHw {
            name: if info.name.is_empty() { a.name() } else { info.name },
            vendor: info.vendor,
            device_type: info.device_type,
            vram_bytes,
            unified,
            max_texture_dim: if info.max_texture_dim == 0 { 8192 } else { info.max_texture_dim },
            native: !effectcraft_render::passes::is_deferred(Some(a)),
        }
    });
    Hardware { gpu, threads: rayon::current_num_threads(), ram_bytes: host.ram_bytes, avail_ram_bytes: host.avail_ram_bytes, core_kinds: host.core_kinds }
}

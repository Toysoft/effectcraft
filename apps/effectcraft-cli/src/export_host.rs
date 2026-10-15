//! Host facts for the export tuning (`effectcraft_export::tuning::Host`): the export library
//! probes nothing itself, so the CLI measures RAM, core kinds and VRAM here (process spawns
//! and system files) and passes them in.

use effectcraft_export::tuning::Host;

/// RAM, core kinds and dedicated VRAM of this machine; unknown values stay `None`.
pub fn probe() -> Host {
    let (ram_bytes, avail_ram_bytes) = memory();
    Host { ram_bytes, avail_ram_bytes, core_kinds: core_kinds(), nvidia_vram_bytes: nvidia_smi_vram_mb().map(|mb| mb << 20), other_vram_bytes: sysfs_vram() }
}

/// `sysctl -n NAME` (macOS), parsed.
#[cfg(target_os = "macos")]
fn sysctl(name: &str) -> Option<u64> {
    let out = std::process::Command::new("/usr/sbin/sysctl").args(["-n", name]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// Physical memory and memory available now (free + inactive + speculative + purgeable pages on
/// macOS, `MemAvailable` on Linux); `None` where not queried.
fn memory() -> (Option<u64>, Option<u64>) {
    #[cfg(target_os = "macos")]
    {
        let total = sysctl("hw.memsize");
        let avail = std::process::Command::new("/usr/bin/vm_stat").output().ok().and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout).into_owned();
            let page = text.split("page size of ").nth(1)?.split_whitespace().next()?.parse::<u64>().ok()?;
            let pages = |key: &str| {
                text.lines()
                    .find(|l| l.starts_with(key))
                    .and_then(|l| l.rsplit(':').next())
                    .and_then(|v| v.trim().trim_end_matches('.').parse::<u64>().ok())
                    .unwrap_or(0)
            };
            let n = pages("Pages free") + pages("Pages inactive") + pages("Pages speculative") + pages("Pages purgeable");
            Some(n.saturating_mul(page))
        });
        (total, avail)
    }
    #[cfg(target_os = "linux")]
    {
        let Ok(m) = std::fs::read_to_string("/proc/meminfo") else { return (None, None) };
        let kb =
            |key: &str| m.lines().find(|l| l.starts_with(key)).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse::<u64>().ok()).map(|v| v * 1024);
        (kb("MemTotal:"), kb("MemAvailable:"))
    }
    #[cfg(windows)]
    {
        // No platform calls (the workspace has no `unsafe`): ask CIM through PowerShell, in KB.
        let out = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$o = Get-CimInstance Win32_OperatingSystem; \"$($o.TotalVisibleMemorySize) $($o.FreePhysicalMemory)\"",
            ])
            .output();
        let Ok(out) = out else { return (None, None) };
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let mut kb = text.split_whitespace().map(|v| v.parse::<u64>().ok().map(|v| v * 1024));
        (kb.next().flatten(), kb.next().flatten())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        (None, None)
    }
}

/// Performance and efficiency cores (Apple silicon's `hw.perflevel0/1`); `None` elsewhere.
fn core_kinds() -> Option<(usize, usize)> {
    #[cfg(target_os = "macos")]
    {
        let p = sysctl("hw.perflevel0.logicalcpu")? as usize;
        let e = sysctl("hw.perflevel1.logicalcpu").unwrap_or(0) as usize;
        Some((p, e))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Dedicated VRAM from Linux sysfs (amdgpu reports it; NVIDIA's proprietary driver and Intel do
/// not); `None` elsewhere. NVIDIA (Windows and Linux): [`nvidia_smi_vram_mb`], which ships with
/// the driver. No platform calls: the workspace has no `unsafe`.
fn sysfs_vram() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let dir = std::fs::read_dir("/sys/class/drm").ok()?;
        dir.filter_map(|e| e.ok())
            .filter_map(|e| std::fs::read_to_string(e.path().join("device/mem_info_vram_total")).ok())
            .filter_map(|t| t.trim().parse::<u64>().ok())
            .max()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Total memory of the largest NVIDIA GPU in MB, from `nvidia-smi`; `None` without it.
fn nvidia_smi_vram_mb() -> Option<u64> {
    let out = std::process::Command::new("nvidia-smi").args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.trim().parse::<u64>().ok()).max()
}

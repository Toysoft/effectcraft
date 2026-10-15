//! Process-wide switches the export tuning (`effectcraft_export::tuning`) hands to the GPU
//! compositor before a render, so the GPU crate reads no environment of its own, plus the
//! reason the accelerator last declined a frame on this thread (logged by the export).
//!
//! Defaults are the compositor's normal behaviour; the export only turns them off when asked.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

static PRECOMP_RESIDENT: AtomicBool = AtomicBool::new(true);
static UNIFIED: AtomicBool = AtomicBool::new(false);

/// The switches (see the export's `TUNING.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuFlags {
    /// Render precomp layers' nested comps resident on the GPU.
    pub precomp_resident: bool,
    /// The GPU shares system memory (re-uploads are cheap copies: a full upload cache is not
    /// worth a warning).
    pub unified: bool,
}

impl Default for GpuFlags {
    fn default() -> Self {
        GpuFlags { precomp_resident: true, unified: false }
    }
}

pub fn set(f: GpuFlags) {
    PRECOMP_RESIDENT.store(f.precomp_resident, Ordering::Relaxed);
    UNIFIED.store(f.unified, Ordering::Relaxed);
}

pub fn get() -> GpuFlags {
    GpuFlags { precomp_resident: PRECOMP_RESIDENT.load(Ordering::Relaxed), unified: UNIFIED.load(Ordering::Relaxed) }
}

thread_local! {
    static DECLINE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The accelerator declined the frame this thread asked for, because `why`.
pub fn note_decline(why: impl Into<String>) {
    DECLINE.with(|d| *d.borrow_mut() = Some(why.into()));
}

/// The reason noted by [`note_decline`] since the last call.
pub fn take_decline() -> Option<String> {
    DECLINE.with(|d| d.borrow_mut().take())
}

//! The streaming frame scheduler of a [`Plan::Scheduled`](crate::tuning::Plan) export.
//!
//! Workers (GPU and CPU compositor) take output frames one at a time from a shared [`Queue`]
//! and hand the results back; the encoder takes them in frame order as soon as the next one is
//! ready. At most `cap` frames are claimed ahead of the encoder (in flight or waiting in the
//! reorder buffer), so memory stays bounded and workers block while the window is full.
//!
//! The number of CPU workers is either fixed or chosen by [`Adapt`]: start GPU-only, measure
//! frames/s over a window, add one CPU worker at a time while throughput improves, and drop
//! back to the best count (then stop adapting) once it doesn't.

use std::collections::BTreeMap;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Duration;

struct Q<T> {
    next_claim: usize,
    next_out: usize,
    ready: BTreeMap<usize, T>,
    /// CPU workers allowed to take frames (ids `0..cpu_active`).
    cpu_active: usize,
    /// Workers still running (a worker that panics counts down too).
    alive: usize,
    stop: bool,
}

/// [`Queue::try_claim`]'s answer.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Claim {
    Got(usize),
    /// The window is full (a frame before it is not done).
    Full,
    /// Nothing left for this worker.
    Done,
}

/// Work queue + bounded reorder buffer for `n` frames.
pub(crate) struct Queue<T> {
    q: Mutex<Q<T>>,
    cv: Condvar,
    n: usize,
    cap: usize,
}

/// Counts a worker out of [`Queue`] when dropped (also on panic).
pub(crate) struct Alive<'q, T>(&'q Queue<T>);

impl<T> Drop for Alive<'_, T> {
    fn drop(&mut self) {
        let mut q = self.0.lock();
        q.alive = q.alive.saturating_sub(1);
        self.0.cv.notify_all();
    }
}

impl<T> Queue<T> {
    pub fn new(n: usize, cap: usize, workers: usize, cpu_active: usize) -> Queue<T> {
        let q = Q { next_claim: 0, next_out: 0, ready: BTreeMap::new(), cpu_active, alive: workers, stop: false };
        Queue { q: Mutex::new(q), cv: Condvar::new(), n, cap: cap.max(1) }
    }

    fn lock(&self) -> MutexGuard<'_, Q<T>> {
        self.q.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The guard a worker holds while it runs.
    pub fn alive(&self) -> Alive<'_, T> {
        Alive(self)
    }

    /// The next frame for a worker: a GPU worker (`cpu: None`) or CPU worker `cpu: Some(id)`.
    /// Blocks while the window is full or (CPU) the worker is not active. A CPU worker leaves
    /// the last `cpu_active` frames to the GPU when `gpu_workers` > 0. `None`: nothing left.
    pub fn claim(&self, cpu: Option<usize>, gpu_workers: usize) -> Option<usize> {
        match self.claim_with(cpu, gpu_workers, true) {
            Claim::Got(i) => Some(i),
            _ => None,
        }
    }

    /// [`Queue::claim`] that returns [`Claim::Full`] instead of blocking.
    pub fn try_claim(&self, cpu: Option<usize>, gpu_workers: usize) -> Claim {
        self.claim_with(cpu, gpu_workers, false)
    }

    fn claim_with(&self, cpu: Option<usize>, gpu_workers: usize, block: bool) -> Claim {
        let mut q = self.lock();
        loop {
            if q.stop || q.next_claim >= self.n {
                return Claim::Done;
            }
            let mut may = q.next_claim < q.next_out + self.cap;
            if let Some(id) = cpu {
                if gpu_workers > 0 && self.n - q.next_claim <= q.cpu_active {
                    return Claim::Done;
                }
                may &= id < q.cpu_active;
            }
            if may {
                q.next_claim += 1;
                return Claim::Got(q.next_claim - 1);
            }
            if !block {
                return Claim::Full;
            }
            q = self.cv.wait_timeout(q, Duration::from_millis(200)).map(|(g, _)| g).unwrap_or_else(|e| e.into_inner().0);
        }
    }

    /// Frame `i` is done.
    pub fn put(&self, i: usize, v: T) {
        self.lock().ready.insert(i, v);
        self.cv.notify_all();
    }

    /// The next frame in order, waiting for it; `None` when all were taken, on [`Queue::stop`],
    /// or when every worker has exited without producing it.
    pub fn next(&self) -> Option<(usize, T)> {
        let mut q = self.lock();
        loop {
            let i = q.next_out;
            if let Some(v) = q.ready.remove(&i) {
                q.next_out += 1;
                self.cv.notify_all();
                return Some((i, v));
            }
            if i >= self.n || q.stop || q.alive == 0 {
                return None;
            }
            q = self.cv.wait(q).unwrap_or_else(|e| e.into_inner());
        }
    }

    pub fn set_cpu_active(&self, n: usize) {
        self.lock().cpu_active = n;
        self.cv.notify_all();
    }

    /// Stop handing out frames (the encoder failed or was cancelled).
    pub fn stop(&self) {
        self.lock().stop = true;
        self.cv.notify_all();
    }

    /// Frames waiting in the reorder buffer.
    #[cfg(test)]
    fn buffered(&self) -> usize {
        self.lock().ready.len()
    }
}

/// Adaptive CPU worker count from measured throughput.
#[derive(Debug)]
pub(crate) struct Adapt {
    cpu_ms: Option<f64>,
    cap: usize,
    note: Option<String>,
    max: usize,
    pub active: usize,
    best: (f64, usize),
    start: f64,
    frames: usize,
    warm: bool,
    pub settled: bool,
}

impl Adapt {
    /// Improvement needed to count as faster.
    const GAIN: f64 = 1.03;

    pub fn new(max: usize, now: f64) -> Adapt {
        Adapt { max, active: 0, best: (0.0, 0), start: now, frames: 0, warm: false, settled: max == 0, cpu_ms: None, cap: 0, note: None }
    }

    /// An estimate of one CPU frame (ms) and the reorder buffer's capacity: a CPU worker is
    /// not even tried when it cannot pay off. Its frames must be ready before the GPU fills the
    /// reorder buffer (else the in-order encoder and then the GPU wait for it), and it must add
    /// more than [`Adapt::GAIN`].
    pub fn with_cpu_estimate(mut self, cpu_ms: f64, cap: usize) -> Adapt {
        self.cpu_ms = Some(cpu_ms);
        self.cap = cap;
        self
    }

    /// Why the adaptation stopped without trying a worker (taken once).
    pub fn take_note(&mut self) -> Option<String> {
        self.note.take()
    }

    /// Frames measured at `active` CPU workers (more workers: longer, their frames are slower).
    fn window(&self) -> usize {
        8 + 4 * self.active
    }

    /// A frame reached the encoder at `now` (seconds). Returns the new CPU worker count when it
    /// changes, with the measured frames/s.
    pub fn frame(&mut self, now: f64) -> Option<(usize, f64)> {
        if self.settled {
            return None;
        }
        self.frames += 1;
        // The first window warms up (shader compiles, caches) and is not measured; so are the
        // first frames after a change (the new worker's first frame is still in flight).
        let need = if self.warm { self.window() } else { 4 };
        if self.frames < need {
            return None;
        }
        let fps = self.frames as f64 / (now - self.start).max(1e-9);
        self.start = now;
        self.frames = 0;
        if !self.warm {
            self.warm = true;
            return None;
        }
        if fps > self.best.0 * Self::GAIN {
            self.best = (fps, self.active);
            if self.active == 0
                && let Some(c) = self.cpu_ms
            {
                let gpu_ms = 1000.0 / fps.max(1e-9);
                let gain = gpu_ms / c;
                let stall = c > 0.5 * self.cap as f64 * gpu_ms;
                if gain < Self::GAIN - 1.0 || stall {
                    self.settled = true;
                    self.note = Some(format!(
                        "no CPU workers: a CPU frame takes ~{c:.0} ms vs {gpu_ms:.0} ms per GPU frame ({}), so a CPU worker would {}",
                        if stall { format!("over half the {}-frame reorder buffer", self.cap) } else { format!("+{:.1}% at best", gain * 100.0) },
                        if stall { "stall the in-order encoder" } else { "not help" }
                    ));
                    return None;
                }
            }
            if self.active < self.max {
                self.active += 1;
                return Some((self.active, fps));
            }
            self.settled = true;
            return None;
        }
        self.settled = true;
        self.active = self.best.1;
        Some((self.active, fps))
    }
}

/// Frames a GPU worker keeps in flight, lowered when the GPU runs out of memory.
///
/// The GPU crate prefixes a decline caused by a failed allocation with
/// [`OUT_OF_MEMORY`]; the device stays usable, so the worker lets its in-flight frames finish,
/// keeps one frame fewer in flight from then on (never below 1) and retries the frame on the
/// GPU once before the CPU renders it.
#[derive(Debug)]
pub(crate) struct Inflight {
    pub(crate) limit: usize,
    lowered: bool,
}

/// How a GPU decline caused by a failed allocation starts (see `effectcraft_gpu::OUT_OF_MEMORY`).
pub(crate) const OUT_OF_MEMORY: &str = "out of memory";

/// What to do with a frame the GPU declined.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum OnDecline {
    /// Out of memory: wait for the frames in flight, then retry on the GPU. `lowered` is the
    /// new limit the first time it drops (log it once), `None` afterwards.
    Retry { lowered: Option<(usize, usize)> },
    /// Render it on the CPU.
    Cpu,
}

impl Inflight {
    pub(crate) fn new(limit: usize) -> Self {
        Self { limit: limit.max(1), lowered: false }
    }

    /// A frame was declined because `why`; `retried`: this frame was already retried.
    pub(crate) fn declined(&mut self, why: Option<&str>, retried: bool) -> OnDecline {
        if retried || !why.is_some_and(|w| w.starts_with(OUT_OF_MEMORY)) {
            return OnDecline::Cpu;
        }
        let was = self.limit;
        self.limit = (was - 1).max(1);
        let first = self.limit < was && !self.lowered;
        self.lowered |= self.limit < was;
        OnDecline::Retry { lowered: first.then_some((was, self.limit)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Workers finishing out of order still stream in order, and never more than `cap`
    /// frames are claimed ahead of the consumer.
    #[test]
    fn ordered_and_bounded() {
        let (n, cap) = (200, 6);
        let q = Queue::<usize>::new(n, cap, 4, 3);
        let max_ahead = AtomicUsize::new(0);
        let out = std::thread::scope(|s| {
            for w in 0..4usize {
                let (q, max_ahead) = (&q, &max_ahead);
                s.spawn(move || {
                    let _a = q.alive();
                    let cpu = w.checked_sub(1);
                    while let Some(i) = q.claim(cpu, 1) {
                        std::thread::sleep(Duration::from_micros(((i * 7919 + w * 31) % 13) as u64 * 50));
                        max_ahead.fetch_max(q.buffered() + 1, Ordering::Relaxed);
                        q.put(i, i * 10);
                    }
                });
            }
            let mut out = vec![];
            while let Some((i, v)) = q.next() {
                std::thread::sleep(Duration::from_micros(30));
                out.push((i, v));
            }
            out
        });
        assert_eq!(out.len(), n);
        assert!(out.iter().enumerate().all(|(k, &(i, v))| i == k && v == k * 10));
        assert!(max_ahead.load(Ordering::Relaxed) <= cap, "buffer exceeded cap");
    }

    /// A slow consumer blocks the workers (backpressure).
    #[test]
    fn backpressure() {
        let q = Queue::<usize>::new(100, 4, 1, 0);
        std::thread::scope(|s| {
            s.spawn(|| {
                let _a = q.alive();
                while let Some(i) = q.claim(None, 1) {
                    q.put(i, i);
                }
            });
            std::thread::sleep(Duration::from_millis(100));
            assert_eq!(q.buffered(), 4);
            for k in 0..100 {
                assert_eq!(q.next().map(|p| p.0), Some(k));
            }
            assert!(q.next().is_none());
        });
    }

    /// Inactive CPU workers wait; stop() and a dead worker pool release everyone.
    #[test]
    fn cpu_gate_stop_and_dead_workers() {
        let q = Queue::<usize>::new(50, 8, 1, 0);
        std::thread::scope(|s| {
            let h = s.spawn(|| {
                let _a = q.alive();
                q.claim(Some(0), 0)
            });
            std::thread::sleep(Duration::from_millis(50));
            assert!(!h.is_finished(), "inactive CPU worker took a frame");
            q.set_cpu_active(1);
            assert_eq!(h.join().ok().flatten(), Some(0));
        });
        // The only worker exited without frame 1: next() gives up instead of hanging.
        q.put(0, 0);
        assert_eq!(q.next().map(|p| p.0), Some(0));
        assert!(q.next().is_none());
        let q2 = Queue::<usize>::new(10, 2, 1, 0);
        q2.stop();
        assert!(q2.claim(None, 1).is_none());
    }

    /// CPU workers leave the tail to the GPU.
    #[test]
    fn tail_left_to_gpu() {
        let q = Queue::<usize>::new(5, 8, 2, 2);
        assert_eq!(q.claim(Some(0), 1), Some(0));
        assert_eq!(q.claim(Some(1), 1), Some(1));
        assert_eq!(q.claim(Some(0), 1), Some(2));
        assert_eq!(q.claim(Some(0), 1), None);
        assert_eq!(q.claim(None, 1), Some(3));
        assert_eq!(q.claim(Some(0), 0), Some(4));
    }

    /// Simulated throughput: adds workers while it helps, drops back when it hurts.
    fn run(max: usize, fps_at: impl Fn(usize) -> f64) -> Adapt {
        let mut a = Adapt::new(max, 0.0);
        let mut t = 0.0;
        for _ in 0..400 {
            t += 1.0 / fps_at(a.active);
            a.frame(t);
        }
        a
    }

    #[test]
    fn adapt_choices() {
        let a = run(4, |n| [0.76, 0.9, 0.98, 0.97, 0.8][n]);
        assert_eq!((a.active, a.settled), (2, true));
        let a = run(4, |n| [0.76, 0.6, 0.5, 0.4, 0.3][n]);
        assert_eq!((a.active, a.settled), (0, true));
        let a = run(3, |n| 0.5 + n as f64 * 0.2);
        assert_eq!((a.active, a.settled), (3, true));
        let a = run(0, |_| 1.0);
        assert_eq!(a.active, 0);
    }
}

#[cfg(test)]
mod estimate_tests {
    use super::Adapt;

    fn feed(mut a: Adapt, fps: f64) -> Adapt {
        let mut t = 0.0;
        for _ in 0..40 {
            t += 1.0 / fps;
            a.frame(t);
            if a.settled {
                break;
            }
        }
        a
    }

    #[test]
    fn slow_cpu_frames_are_not_tried() {
        // An RTX 3090 at 4K: GPU ~9.5 fps, CPU frame ~2.4 s, 16-frame buffer: skip.
        let mut a = feed(Adapt::new(8, 0.0).with_cpu_estimate(2400.0, 16), 9.5);
        assert!(a.settled && a.active == 0);
        assert!(a.take_note().unwrap().contains("stall"));
        // A slow GPU next to a fast CPU: a worker is tried (no skip note).
        let mut a = feed(Adapt::new(8, 0.0).with_cpu_estimate(300.0, 16), 1.0);
        assert!(a.take_note().is_none());
    }
}

#[cfg(test)]
mod inflight_tests {
    use super::{Inflight, OnDecline};

    #[test]
    fn out_of_memory_lowers_inflight_once_and_retries_on_the_gpu() {
        let mut f = Inflight::new(3);
        let oom = Some("out of memory: Not enough memory left");
        assert_eq!(f.declined(oom, false), OnDecline::Retry { lowered: Some((3, 2)) });
        assert_eq!(f.limit, 2);
        // Lowered again, but logged only the first time.
        assert_eq!(f.declined(oom, false), OnDecline::Retry { lowered: None });
        assert_eq!(f.limit, 1);
        // Never below 1; still retried once.
        assert_eq!(f.declined(oom, false), OnDecline::Retry { lowered: None });
        assert_eq!(f.limit, 1);
        // A retried frame that fails again goes to the CPU.
        assert_eq!(f.declined(oom, true), OnDecline::Cpu);
    }

    #[test]
    fn other_declines_go_to_the_cpu() {
        let mut f = Inflight::new(2);
        assert_eq!(f.declined(Some("device unhealthy: GPU device lost"), false), OnDecline::Cpu);
        assert_eq!(f.declined(Some("device failure / out of memory: uncaptured GPU error"), false), OnDecline::Cpu);
        assert_eq!(f.declined(None, false), OnDecline::Cpu);
        assert_eq!(f.limit, 2);
        assert_eq!(Inflight::new(0).limit, 1);
    }
}

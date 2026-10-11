//! The console's snapshots: the composition's current frame at the viewer's resolution, saved
//! to a gallery folder (PNG keeps the transparency, or JPEG), listed newest first, copied to the
//! clipboard (the frontend), exported, imported back into the project or deleted. The gallery is
//! a scratch place for comparing design ideas, not an archive: keep what matters elsewhere.

use std::path::{Path, PathBuf};

use effectcraft_project::ItemId;
use effectcraft_render::{Backend, RenderOpts};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

/// The default gallery folder's name (next to the settings).
pub const FOLDER_NAME: &str = "Console Effect Gallery";
/// Longest snapshot file name accepted from a command.
const MAX_NAME_BYTES: usize = 255;
/// Largest image decoded when exporting to another format (pixels per side).
const MAX_DECODE_SIDE: u32 = 16_384;

/// Snapshot file formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpg,
}

impl Format {
    pub fn parse(s: &str) -> Option<Format> {
        match s.trim().trim_start_matches('.').to_ascii_lowercase().as_str() {
            "png" => Some(Format::Png),
            "jpg" | "jpeg" => Some(Format::Jpg),
            _ => None,
        }
    }
    /// The format a file name's extension names.
    pub fn of_path(p: &str) -> Option<Format> {
        Path::new(p).extension().and_then(|e| Format::parse(&e.to_string_lossy()))
    }
    pub fn ext(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpg => "jpg",
        }
    }
}

/// One snapshot in the gallery.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shot {
    /// File name (what the `consoleEffect.*Snapshots` commands take).
    pub name: String,
    pub path: String,
    pub bytes: u64,
    /// Modification time, milliseconds since 1970 (0 when unknown).
    pub modified: u64,
}

#[cfg(not(target_arch = "wasm32"))]
fn temp_folder() -> Option<PathBuf> {
    Some(std::env::temp_dir().join(format!("EffectCraft {FOLDER_NAME}")))
}

// (The browser has no temporary folder; its settings store names one.)
#[cfg(target_arch = "wasm32")]
fn temp_folder() -> Option<PathBuf> {
    None
}

/// The default gallery folder: next to the settings, else in the system's temporary folder.
pub fn default_folder(s: &Session) -> Option<PathBuf> {
    s.config.as_ref().and_then(|c| c.dir()).map(|d| d.join(FOLDER_NAME)).or_else(temp_folder)
}

/// The gallery folder: the setting, else [`default_folder`].
pub fn folder(s: &Session) -> Option<PathBuf> {
    let set = s.console_effect.gallery_folder.trim();
    if set.is_empty() { default_folder(s) } else { Some(PathBuf::from(set)) }
}

/// The snapshots, newest first.
pub fn list(s: &Session) -> Vec<Shot> {
    let Some(dir) = folder(s) else { return vec![] };
    let mut v: Vec<Shot> = s
        .services
        .list_dir(&dir.to_string_lossy())
        .into_iter()
        .filter(|p| Format::of_path(p).is_some())
        .filter_map(|p| {
            let name = Path::new(&p).file_name()?.to_string_lossy().to_string();
            let (bytes, ns) = s.services.stamp(&p).unwrap_or((0, 0));
            Some(Shot { name, path: p, bytes, modified: u64::try_from(ns / 1_000_000).unwrap_or(u64::MAX) })
        })
        .collect();
    v.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| b.name.cmp(&a.name)));
    v
}

/// The path of snapshot `name` in the gallery: a plain file name of a snapshot that is there
/// (never a path, so a command can't reach files outside the gallery).
pub fn resolve(s: &Session, name: &str) -> std::result::Result<PathBuf, String> {
    let plain = !name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '\0'])
        && Path::new(name).file_name().is_some_and(|n| n == name);
    if !plain || Format::of_path(name).is_none() {
        return Err(format!("`{name}` is not a snapshot file name (see consoleEffect.gallery)"));
    }
    let dir = folder(s).ok_or("there is no gallery folder here")?;
    let path = dir.join(name);
    if !s.services.exists(&path.to_string_lossy()) {
        return Err(format!("no snapshot `{name}` in the gallery"));
    }
    Ok(path)
}

/// The current frame of comp `cid` as 8-bit RGBA at `scale` (the viewer's resolution): with its
/// transparency (`alpha`) or over the composition's background colour.
pub fn frame_rgba(s: &Session, cid: ItemId, scale: f64, alpha: bool) -> Result<(u32, u32, Vec<u8>)> {
    let comp = s.project.comp(cid).ok_or(EngineError::NoComp)?;
    let scale = if scale.is_finite() { scale.clamp(0.05, 1.0) } else { 1.0 };
    let t = s.time_of(cid);
    let opts = RenderOpts { scale, backend: Backend::Auto, view: s.view_camera(cid).filter(|_| comp.has_3d()), ..Default::default() };
    let img = s.render(cid, t, opts);
    let rgba = if alpha { img.to_rgba8() } else { img.to_rgba8_over(comp.background) };
    Ok((img.width, img.height, rgba))
}

/// Encode straight 8-bit RGBA as `format` (JPEG drops the alpha: pass opaque pixels).
pub fn encode(rgba: &[u8], w: u32, h: u32, format: Format, quality: u8) -> std::result::Result<Vec<u8>, String> {
    let expected = (w as usize).checked_mul(h as usize).and_then(|n| n.checked_mul(4)).ok_or("the image is too large")?;
    if rgba.len() != expected || w == 0 || h == 0 {
        return Err(format!("{} bytes are not a {w} × {h} RGBA image", rgba.len()));
    }
    match format {
        Format::Png => crate::commands::comp_more::encode_png(rgba, w, h),
        Format::Jpg => {
            let rgb: Vec<u8> = rgba.as_chunks::<4>().0.iter().flat_map(|&[r, g, b, _]| [r, g, b]).collect();
            let mut out = vec![];
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100))
                .encode(&rgb, w, h, image::ExtendedColorType::Rgb8)
                .map_err(|e| e.to_string())?;
            Ok(out)
        }
    }
}

/// Decode a PNG or JPEG snapshot to straight 8-bit RGBA (bounded size).
pub fn decode(bytes: &[u8]) -> std::result::Result<(u32, u32, Vec<u8>), String> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DECODE_SIDE);
    limits.max_image_height = Some(MAX_DECODE_SIDE);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| e.to_string())?.to_rgba8();
    Ok((img.width(), img.height(), img.into_raw()))
}

/// Straight RGBA flattened over white (a transparent snapshot exported as JPEG).
fn over_white(rgba: &mut [u8]) {
    for [r, g, b, a] in rgba.as_chunks_mut::<4>().0 {
        let k = u32::from(*a);
        for c in [r, g, b] {
            *c = ((u32::from(*c) * k + 255 * (255 - k) + 127) / 255) as u8;
        }
        *a = 255;
    }
}

/// UTC calendar date and time of `secs` since 1970: (year, month, day, hour, minute, second).
pub fn civil(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    // Days to the civil calendar (proleptic Gregorian), after Howard Hinnant's algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let n = |v: i64| u32::try_from(v).unwrap_or(0);
    (y, n(m), n(d), n(i64::try_from(rem / 3_600).unwrap_or(0)), n(i64::try_from(rem % 3_600 / 60).unwrap_or(0)), n(i64::try_from(rem % 60).unwrap_or(0)))
}

/// A file name for a snapshot of `comp` at `frame`, taken `secs` after 1970:
/// `Comp 1 00012 2026-10-11 21.55.03` (UTC).
pub fn stem(comp: &str, frame: i64, secs: u64) -> String {
    let safe: String = comp.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c }).take(64).collect();
    let safe = safe.trim().trim_matches('.').trim();
    let safe = if safe.is_empty() { "Comp" } else { safe };
    let (y, mo, d, h, mi, s) = civil(secs);
    format!("{safe} {frame:05} {y:04}-{mo:02}-{d:02} {h:02}.{mi:02}.{s:02}")
}

/// A path in `dir` for `stem.ext` that no file has yet (` 2`, ` 3`… appended).
fn unique(s: &Session, dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !s.services.exists(&first.to_string_lossy()) {
        return first;
    }
    (2..10_000).map(|n| dir.join(format!("{stem} {n}.{ext}"))).find(|p| !s.services.exists(&p.to_string_lossy())).unwrap_or(first)
}

/// Seconds since 1970 now.
fn now_secs() -> u64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Snapshot comp `cid`: render its current frame at `scale` (with transparency when `alpha`
/// and the format keeps it) and write it to `out`, or into the gallery under a new name.
pub fn snapshot(s: &mut Session, cid: ItemId, scale: f64, alpha: bool, format: Format, out: Option<&str>) -> Result<Value> {
    let (name, frame, t) = {
        let comp = s.project.comp(cid).ok_or(EngineError::NoComp)?;
        let t = s.time_of(cid);
        let frame = (t.seconds() * comp.frame_rate.as_f64()).round() as i64;
        (s.project.item(cid).map(|i| i.name.clone()).unwrap_or_default(), frame, t)
    };
    let keep_alpha = alpha && format == Format::Png;
    let (w, h, rgba) = frame_rgba(s, cid, scale, keep_alpha)?;
    let bytes = encode(&rgba, w, h, format, s.console_effect.jpeg_quality).map_err(|e| EngineError::Other(format!("cannot encode the snapshot: {e}")))?;
    let (path, gallery) = match out {
        Some(p) => (PathBuf::from(p), false),
        None => {
            let dir = folder(s).ok_or_else(|| EngineError::Other("there is no gallery folder here: set one in Console Effect Settings".into()))?;
            (unique(s, &dir, &stem(&name, frame, now_secs()), format.ext()), true)
        }
    };
    let ps = path.to_string_lossy().to_string();
    let written = if gallery { s.services.store_file(&ps, &bytes) } else { s.services.write_file(&ps, &bytes) };
    written.map_err(|e| EngineError::Other(format!("cannot write {ps}: {e}")))?;
    let file = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Ok(json!({
        "path": ps,
        "name": file,
        "width": w,
        "height": h,
        "time": t.seconds(),
        "frame": frame,
        "alpha": keep_alpha,
        "format": format.ext(),
        "gallery": gallery,
    }))
}

/// Delete snapshots by file name. Returns how many went.
pub fn delete(s: &mut Session, names: &[String]) -> std::result::Result<usize, String> {
    let paths = names.iter().map(|n| resolve(s, n)).collect::<std::result::Result<Vec<_>, _>>()?;
    let mut n = 0;
    for p in paths {
        s.services.remove_file(&p.to_string_lossy()).map_err(|e| format!("cannot delete {}: {e}", p.display()))?;
        n += 1;
    }
    Ok(n)
}

/// Copy snapshots into `dest` (as they are, or converted to `format`). Returns the new paths.
pub fn export(s: &mut Session, names: &[String], dest: &str, format: Option<Format>, quality: u8) -> std::result::Result<Vec<String>, String> {
    let paths = names.iter().map(|n| resolve(s, n)).collect::<std::result::Result<Vec<_>, _>>()?;
    let dest = Path::new(dest);
    let mut out = vec![];
    for p in paths {
        let ps = p.to_string_lossy().to_string();
        let bytes = s.services.read_file(&ps).map_err(|e| format!("cannot read {ps}: {e}"))?;
        let own = Format::of_path(&ps).unwrap_or(Format::Png);
        let to = format.unwrap_or(own);
        let stem = p.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Snapshot".into());
        let bytes = if to == own {
            bytes
        } else {
            let (w, h, mut rgba) = decode(&bytes).map_err(|e| format!("cannot read {ps}: {e}"))?;
            if to == Format::Jpg {
                over_white(&mut rgba);
            }
            encode(&rgba, w, h, to, quality)?
        };
        let target = dest.join(format!("{stem}.{}", to.ext()));
        let ts = target.to_string_lossy().to_string();
        s.services.write_file(&ts, &bytes).map_err(|e| format!("cannot write {ts}: {e}"))?;
        out.push(ts);
    }
    Ok(out)
}

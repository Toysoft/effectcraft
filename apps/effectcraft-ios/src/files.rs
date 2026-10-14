//! iOS Files pickers. The editor keeps durable local copies and exports completed writes.
use std::collections::{HashSet, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use effectcraft_ui_egui::{DialogDone, FileDialog};

type Errors = Arc<Mutex<Option<String>>>;
#[derive(Default)]
struct Writes {
    staged: HashSet<PathBuf>,
    ready: VecDeque<PathBuf>,
}

pub struct Files {
    root: PathBuf,
    writes: Arc<Mutex<Writes>>,
    native_error: Errors,
    exporting: Arc<AtomicBool>,
}

fn save_path(dir: &Path, name: &str, ext: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    if name.is_empty() || name.contains(['/', '\\']) || !matches!(Path::new(name).components().next(), Some(Component::Normal(_))) {
        return Err("Enter a file name without folders.".into());
    }
    let mut path = dir.join(name);
    if !ext.is_empty() && path.extension().is_none() {
        path.set_extension(ext);
    }
    Ok(path)
}

/// Every Save As keeps a distinct local copy, even if its Files export is canceled.
fn stage_save(root: &Path, default: &str, ext: &str) -> Result<PathBuf, String> {
    let name = Path::new(default).file_name().and_then(|n| n.to_str()).ok_or("The suggested file name is invalid")?;
    let saves = root.join("Saved");
    std::fs::create_dir_all(&saves).map_err(|e| format!("Cannot create Saved folder: {e}"))?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    for attempt in 0..128 {
        let dir = saves.join(format!("{stamp}-{attempt}"));
        match std::fs::create_dir(&dir) {
            Ok(()) => return save_path(&dir, name, ext),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Cannot create save folder: {e}")),
        }
    }
    Err("Cannot create a unique save location; try again.".into())
}

impl Files {
    pub fn new(root: PathBuf) -> Self {
        Self { root, writes: Arc::default(), native_error: Arc::default(), exporting: Arc::default() }
    }

    pub fn hook(&self, ctx: &egui::Context) -> Box<dyn Fn(FileDialog, DialogDone)> {
        let root = self.root.clone();
        let writes = self.writes.clone();
        let errors = self.native_error.clone();
        let ctx = ctx.clone();
        let exporting = self.exporting.clone();
        Box::new(move |dialog, done| {
            if exporting.load(Ordering::Acquire) {
                *errors.lock().unwrap_or_else(PoisonError::into_inner) = Some("Finish the current Save to Files dialog first.".into());
                done(vec![]);
                ctx.request_repaint();
                return;
            }
            match dialog {
                FileDialog::Save { default, ext, .. } => match stage_save(&root, &default, &ext) {
                    Ok(path) => {
                        writes.lock().unwrap_or_else(PoisonError::into_inner).staged.insert(path.clone());
                        done(vec![path.to_string_lossy().into_owned()]);
                    }
                    Err(e) => {
                        *errors.lock().unwrap_or_else(PoisonError::into_inner) = Some(e);
                        done(vec![]);
                    }
                },
                #[cfg(target_os = "ios")]
                other => open_native(other, root.clone(), done, errors.clone(), ctx.clone()),
                #[cfg(not(target_os = "ios"))]
                _ => {
                    *errors.lock().unwrap_or_else(PoisonError::into_inner) = Some("The native Files picker requires iOS.".into());
                    done(vec![]);
                }
            }
            ctx.request_repaint();
        })
    }

    /// Commands signal completion explicitly: a render's partially written output is never exported.
    pub fn write_hook(&self, ctx: &egui::Context) -> Box<dyn Fn(&str, Result<(), String>)> {
        let writes = self.writes.clone();
        let errors = self.native_error.clone();
        let ctx = ctx.clone();
        Box::new(move |path, result| {
            let path = PathBuf::from(path);
            let mut writes = writes.lock().unwrap_or_else(PoisonError::into_inner);
            if !writes.staged.remove(&path) {
                return;
            }
            match result {
                Ok(()) => writes.ready.push_back(path),
                Err(e) => *errors.lock().unwrap_or_else(PoisonError::into_inner) = Some(format!("Could not save: {e}. No Files export was started.")),
            }
            ctx.request_repaint();
        })
    }

    /// A queue item reached Done: export a movie, or the complete directory of sequence frames.
    pub fn render_completed(&self, staged: &str, actual: &str, sequence: bool) {
        let staged = PathBuf::from(staged);
        let actual = PathBuf::from(actual);
        if !actual.exists() {
            return;
        }
        let mut writes = self.writes.lock().unwrap_or_else(PoisonError::into_inner);
        if writes.staged.remove(&staged) {
            let source = if sequence { staged.parent().filter(|parent| actual.starts_with(parent)).map(Path::to_path_buf).unwrap_or(actual) } else { actual };
            writes.ready.push_back(source);
        }
    }

    pub fn take_error(&self) -> Option<String> {
        self.native_error.lock().unwrap_or_else(PoisonError::into_inner).take()
    }

    pub fn show(&mut self, ctx: &egui::Context, _auto: &mut effectcraft_ui_egui::automation::Registry) {
        #[cfg(target_os = "ios")]
        native::poll();
        #[cfg(target_os = "ios")]
        if !native::busy() && !self.exporting.load(Ordering::Acquire) {
            let path = self.writes.lock().unwrap_or_else(PoisonError::into_inner).ready.pop_front();
            if let Some(path) = path {
                self.exporting.store(true, Ordering::Release);
                export_native(path, self.native_error.clone(), ctx.clone(), self.exporting.clone());
            }
        }
        #[cfg(not(target_os = "ios"))]
        let _ = ctx;
    }
}

/// Copy a picker-owned file to a unique directory without buffering it or replacing media.
#[cfg(any(target_os = "ios", test))]
fn retain_import(source: &Path, root: &Path) -> Result<PathBuf, String> {
    use std::io::{Read, Write};
    use std::time::{SystemTime, UNIX_EPOCH};
    const MAX_BYTES: u64 = 32 * 1024 * 1024 * 1024;
    let mut input = std::fs::File::open(source).map_err(|error| format!("Cannot read selected file: {error}"))?;
    let metadata = input.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err("Choose a regular media file up to 32 GiB.".into());
    }
    let name = source
        .file_name()
        .filter(|name| matches!(Path::new(name).components().next(), Some(Component::Normal(_))))
        .ok_or("The selected file has no valid file name")?;
    let imports = root.join("Imported");
    std::fs::create_dir_all(&imports).map_err(|error| format!("Cannot create Imported folder: {error}"))?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let mut directory = None;
    for attempt in 0..128 {
        let candidate = imports.join(format!("{stamp}-{attempt}"));
        match std::fs::create_dir(&candidate) {
            Ok(()) => {
                directory = Some(candidate);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Cannot create media folder: {error}")),
        }
    }
    let directory = directory.ok_or("Cannot create a unique media folder; try again")?;
    let destination = directory.join(name);
    let result = (|| {
        let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(&destination).map_err(|error| error.to_string())?;
        let bytes = std::io::copy(&mut (&mut input).take(MAX_BYTES + 1), &mut output).map_err(|error| error.to_string())?;
        if bytes > MAX_BYTES || bytes != metadata.len() {
            return Err("The selected file changed while copying; import it again.".into());
        }
        output.flush().map_err(|error| error.to_string())?;
        output.sync_all().map_err(|error| error.to_string())?;
        Ok(destination.clone())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&destination);
        let _ = std::fs::remove_dir(&directory);
    }
    result
}

/// Materialize a selected security-scoped folder while its picker URL is retained.
/// The engine can safely relink from this folder after the app restarts.
#[cfg(any(target_os = "ios", test))]
fn retain_folder(source: &Path, root: &Path) -> Result<PathBuf, String> {
    use std::io::{Read, Write};
    const MAX_BYTES: u64 = 32 * 1024 * 1024 * 1024;
    const MAX_ENTRIES: usize = 4096;
    if !std::fs::symlink_metadata(source).map_err(|e| format!("Cannot read selected folder: {e}"))?.is_dir() {
        return Err("Choose a regular folder in Files.".into());
    }
    let destination = stage_save(root, "Footage folder", "")?;
    let result = (|| {
        std::fs::create_dir(&destination).map_err(|e| e.to_string())?;
        let mut queue = VecDeque::from([(source.to_path_buf(), destination.clone(), 0usize)]);
        let mut count = 0usize;
        let mut total = 0u64;
        while let Some((from, to, depth)) = queue.pop_front() {
            if depth > 16 {
                return Err("Selected folder is nested too deeply (maximum 16 levels).".into());
            }
            for entry in std::fs::read_dir(&from).map_err(|e| format!("Cannot read footage folder: {e}"))? {
                count = count.saturating_add(1);
                if count > MAX_ENTRIES {
                    return Err("Choose a folder with up to 4096 entries.".into());
                }
                let entry = entry.map_err(|e| e.to_string())?;
                let meta = entry.path().symlink_metadata().map_err(|e| e.to_string())?;
                let output = to.join(entry.file_name());
                if meta.is_dir() {
                    std::fs::create_dir(&output).map_err(|e| e.to_string())?;
                    queue.push_back((entry.path(), output, depth.saturating_add(1)));
                } else if meta.is_file() {
                    total = total.checked_add(meta.len()).ok_or("Selected folder is too large")?;
                    if total > MAX_BYTES {
                        return Err("Choose a footage folder up to 32 GiB.".into());
                    }
                    let mut input = std::fs::File::open(entry.path()).map_err(|e| e.to_string())?;
                    let mut output = std::fs::OpenOptions::new().create_new(true).write(true).open(output).map_err(|e| e.to_string())?;
                    let bytes = std::io::copy(&mut (&mut input).take(meta.len().saturating_add(1)), &mut output).map_err(|e| e.to_string())?;
                    if bytes != meta.len() {
                        return Err("Footage changed while copying. Select its folder again.".into());
                    }
                    output.flush().map_err(|e| e.to_string())?;
                    output.sync_all().map_err(|e| e.to_string())?;
                } else {
                    return Err("Footage folders containing symbolic links or special files cannot be copied. Select the original folder instead.".into());
                }
            }
        }
        Ok(destination.clone())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&destination);
    }
    result
}

#[cfg(target_os = "ios")]
fn open_native(dialog: FileDialog, root: PathBuf, done: DialogDone, errors: Errors, ctx: egui::Context) {
    native::open(dialog, root, done, errors, ctx);
}

#[cfg(target_os = "ios")]
// cidre's external class-definition macro emits this redundant conversion internally.
#[allow(clippy::useless_transmute)]
mod native {
    use super::*;
    use cidre::{arc, define_obj_type, ns, objc, objc::Obj, ui, ui::DocumentPickerDelegate, ut};
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct State {
        done: Option<DialogDone>,
        root: PathBuf,
        errors: Errors,
        ctx: egui::Context,
        folder: bool,
        complete: Arc<AtomicBool>,
        scopes: Vec<arc::R<ns::Url>>,
    }
    impl Drop for State {
        fn drop(&mut self) {
            for url in &self.scopes {
                url.stop_accessing_security_scoped_resource();
            }
        }
    }
    define_obj_type!(PickerDelegate + ui::DocumentPickerDelegateImpl, State, EFFECTCRAFT_FILES_DELEGATE);
    impl ui::DocumentPickerDelegate for PickerDelegate {}
    #[objc::add_methods]
    impl ui::DocumentPickerDelegateImpl for PickerDelegate {
        extern "C" fn impl_document_picker_did_pick_documents_at_urls(
            &mut self,
            _cmd: Option<&objc::Sel>,
            _picker: &mut ui::DocumentPickerViewController,
            urls: &ns::Array<ns::Url>,
        ) {
            let state = self.inner_mut();
            let Some(done) = state.done.take() else { return };
            let mut paths = Vec::new();
            if urls.len() > 64 {
                *state.errors.lock().unwrap_or_else(PoisonError::into_inner) = Some("Choose up to 64 files at a time.".into());
                done(vec![]);
                state.complete.store(true, Ordering::Release);
                state.ctx.request_repaint();
                return;
            }
            for url in urls.iter() {
                if state.folder && url.start_accessing_security_scoped_resource() {
                    state.scopes.push(url.retained());
                }
                let Some(path) = url.path() else {
                    *state.errors.lock().unwrap_or_else(PoisonError::into_inner) =
                        Some("Files returned a location that cannot be read as a file. Download it in Files and try again.".into());
                    done(vec![]);
                    state.complete.store(true, Ordering::Release);
                    state.ctx.request_repaint();
                    return;
                };
                paths.push(PathBuf::from(path.to_string()));
            }
            let root = state.root.clone();
            let errors = state.errors.clone();
            let ctx = state.ctx.clone();
            let complete = state.complete.clone();
            let folder = state.folder;
            let completion = Arc::new(Mutex::new(Some(done)));
            let worker_completion = completion.clone();
            let worker_errors = errors.clone();
            let worker_ctx = ctx.clone();
            let worker_complete = complete.clone();
            let result = std::thread::Builder::new().name("EffectCraft Files copy".into()).spawn(move || {
                let result: Result<Vec<String>, String> = paths
                    .into_iter()
                    .map(|path| {
                        let path = if folder { retain_folder(&path, &root)? } else { retain_import(&path, &root)? };
                        Ok(path.to_string_lossy().into_owned())
                    })
                    .collect();
                let paths = match result {
                    Ok(paths) => paths,
                    Err(error) => {
                        *worker_errors.lock().unwrap_or_else(PoisonError::into_inner) = Some(format!("Cannot open selected files: {error}"));
                        vec![]
                    }
                };
                if let Some(done) = worker_completion.lock().unwrap_or_else(PoisonError::into_inner).take() {
                    done(paths);
                }
                worker_complete.store(true, Ordering::Release);
                worker_ctx.request_repaint();
            });
            if let Err(error) = result {
                *errors.lock().unwrap_or_else(PoisonError::into_inner) = Some(format!("Cannot copy selected files: {error}"));
                if let Some(done) = completion.lock().unwrap_or_else(PoisonError::into_inner).take() {
                    done(vec![]);
                }
                complete.store(true, Ordering::Release);
                ctx.request_repaint();
            }
        }
        extern "C" fn impl_document_picker_was_cancelled(&mut self, _cmd: Option<&objc::Sel>, _picker: &mut ui::DocumentPickerViewController) {
            let state = self.inner_mut();
            if let Some(done) = state.done.take() {
                done(vec![]);
            }
            state.complete.store(true, Ordering::Release);
            state.ctx.request_repaint();
        }
    }
    struct Pending {
        _picker: arc::R<ui::DocumentPickerViewController>,
        _delegate: arc::R<PickerDelegate>,
        complete: Arc<AtomicBool>,
    }
    thread_local! { static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) }; }
    pub fn busy() -> bool {
        PENDING.with(|p| p.try_borrow().map(|p| p.is_some()).unwrap_or(true))
    }
    pub fn poll() {
        PENDING.with(|p| {
            let Ok(mut p) = p.try_borrow_mut() else { return };
            if p.as_ref().is_some_and(|p| p.complete.load(Ordering::Acquire)) {
                *p = None;
            }
        });
    }
    fn presenter() -> Result<arc::R<ui::ViewController>, String> {
        let app = ui::App::shared();
        let scene_class = ns::class_from_ns_string::<ui::WindowScene>(&ns::String::with_str("UIWindowScene")).ok_or("UIKit window scenes are unavailable")?;
        for scene in app.connected_scenes().iter() {
            let Some(scene) = scene.try_cast(scene_class) else { continue };
            let Some(window) = scene.key_window() else { continue };
            let Some(root) = window.root_vc() else { continue };
            let mut controller = root.retained();
            for _ in 0..32 {
                match controller.presented_vc() {
                    Some(presented) => controller = presented,
                    None => return Ok(controller),
                }
            }
            return Err("Too many presented windows; close the current dialog first.".into());
        }
        Err("The editor has no active window for Files.".into())
    }
    pub fn open(dialog: FileDialog, root: PathBuf, done: DialogDone, errors: Errors, ctx: egui::Context) {
        poll();
        if busy() {
            *errors.lock().unwrap_or_else(PoisonError::into_inner) = Some("A Files operation is still running; finish it first.".into());
            done(vec![]);
            return;
        }
        let (title, exts, many, folder) = match dialog {
            FileDialog::Open { exts, many } => ("Import into EffectCraft", exts, many, false),
            FileDialog::OpenProject => ("Open EffectCraft project", vec!["ecproj".into(), "ecprojx".into()], false, false),
            FileDialog::Folder => ("Choose footage folder", Vec::new(), false, true),
            FileDialog::Save { .. } => {
                done(vec![]);
                return;
            }
        };
        let mut presenter = match presenter() {
            Ok(p) => p,
            Err(error) => {
                *errors.lock().unwrap_or_else(PoisonError::into_inner) = Some(error);
                done(vec![]);
                return;
            }
        };
        let mut types: Vec<arc::R<ut::Type>> = exts.iter().filter_map(|ext| ut::Type::with_file_ext(&ns::String::with_str(ext))).collect();
        if types.is_empty() {
            types.push(if folder { ut::Type::folder().retained() } else { ut::Type::item().retained() });
        }
        let types = ns::Array::from_slice_retained(&types);
        let mut picker = ui::DocumentPickerViewController::for_opening_content_types(&types, !folder);
        // Full screen keeps the explicit Files Cancel action; dismissing a sheet by swiping
        // would bypass UIDocumentPickerDelegate's completion on some iOS versions.
        picker.set_modal_presentation_style(ui::ModalPresentationStyle::FullScreen);
        picker.set_allows_multiple_selection(many);
        picker.set_should_show_file_extensions(true);
        picker.set_title(Some(&ns::String::with_str(title)));
        let complete = Arc::new(AtomicBool::new(false));
        let mut delegate = PickerDelegate::with(State { done: Some(done), root, errors, ctx, folder, complete: complete.clone(), scopes: Vec::new() });
        picker.set_delegate(Some(delegate.as_ref()));
        let retained = PENDING.with(|p| {
            let Ok(mut p) = p.try_borrow_mut() else { return false };
            *p = Some(Pending { _picker: picker.clone(), _delegate: delegate.clone(), complete });
            true
        });
        if retained {
            presenter.present_vc(&picker, true);
        } else {
            let state = delegate.inner_mut();
            *state.errors.lock().unwrap_or_else(PoisonError::into_inner) = Some("Files is busy; try again after the current dialog finishes.".into());
            if let Some(done) = state.done.take() {
                done(vec![]);
            }
        }
    }
}

#[cfg(target_os = "ios")]
fn export_native(path: PathBuf, errors: Errors, ctx: egui::Context, exporting: Arc<AtomicBool>) {
    if !path.exists() {
        *errors.lock().unwrap_or_else(PoisonError::into_inner) = Some(format!("The completed output is missing: {}", path.display()));
        exporting.store(false, Ordering::Release);
        ctx.request_repaint();
        return;
    }
    let callback_errors = errors.clone();
    let callback_ctx = ctx.clone();
    let copy = path.clone();
    let callback_exporting = exporting.clone();
    // Stream the completed file (or collected folder) through UIKit; never buffer videos in RAM.
    let result = robius_file_picker::FileDialog::new().set_title("Save to Files").save_to_downloads(&path, move |result| {
        match result {
            Ok(Some(_)) => log::info!("Files export completed for {}", copy.display()),
            Ok(None) => {
                *callback_errors.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some(format!("Save to Files canceled. Your local copy is kept at {}.", copy.display()))
            }
            Err(error) => {
                *callback_errors.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some(format!("Cannot save to Files: {error}. Your local copy is kept at {}.", copy.display()))
            }
        }
        callback_exporting.store(false, Ordering::Release);
        callback_ctx.request_repaint();
    });
    if let Err(error) = result {
        exporting.store(false, Ordering::Release);
        *errors.lock().unwrap_or_else(PoisonError::into_inner) =
            Some(format!("Cannot open Save to Files: {error}. Your local copy is kept at {}.", path.display()));
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imported_files_survive_source_removal_and_do_not_overwrite() {
        let root = std::env::temp_dir().join(format!("effectcraft-import-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("image.png");
        std::fs::write(&source, b"original").unwrap();
        let first = retain_import(&source, &root).unwrap();
        std::fs::write(&source, b"second").unwrap();
        let second = retain_import(&source, &root).unwrap();
        std::fs::remove_file(&source).unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read(&first).unwrap(), b"original");
        assert_eq!(std::fs::read(&second).unwrap(), b"second");
        assert!(retain_import(&root, &root).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn staged_outputs_export_only_after_successful_command_completion() {
        let root = std::env::temp_dir().join(format!("effectcraft-save-test-{}", std::process::id()));
        let files = Files::new(root.clone());
        let ctx = egui::Context::default();
        let paths = Arc::new(Mutex::new(Vec::new()));
        let answer = paths.clone();
        files.hook(&ctx)(FileDialog::save("My Project.ecproj"), Box::new(move |p| *answer.lock().unwrap() = p));
        let path = paths.lock().unwrap().first().cloned().unwrap();
        assert!(files.writes.lock().unwrap().ready.is_empty());
        let completed = files.write_hook(&ctx);
        completed("/unrelated/file", Ok(()));
        assert!(files.writes.lock().unwrap().ready.is_empty());
        completed(&path, Err("disk full".into()));
        assert!(files.writes.lock().unwrap().ready.is_empty());
        assert!(files.take_error().unwrap().contains("disk full"));
        let second = stage_save(&root, "My Project.ecproj", "ecproj").unwrap();
        assert_ne!(Path::new(&path), second);
        files.writes.lock().unwrap().staged.insert(second.clone());
        completed(&second.to_string_lossy(), Ok(()));
        assert_eq!(files.writes.lock().unwrap().ready.pop_front(), Some(second));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn active_native_export_rejects_overlapping_requests_without_staging() {
        let files = Files::new(std::env::temp_dir());
        files.exporting.store(true, Ordering::Release);
        let answer = Arc::new(Mutex::new(None));
        let callback_answer = answer.clone();
        files.hook(&egui::Context::default())(FileDialog::save("Overlap.ecproj"), Box::new(move |paths| *callback_answer.lock().unwrap() = Some(paths)));
        assert_eq!(*answer.lock().unwrap(), Some(vec![]));
        assert!(files.writes.lock().unwrap().staged.is_empty());
        assert!(files.take_error().unwrap().contains("Finish"));
    }

    #[test]
    fn selected_folder_preserves_nested_footage_and_rejects_symbolic_links() {
        let root = std::env::temp_dir().join(format!("effectcraft-folder-test-{}", std::process::id()));
        let source = root.join("source");
        std::fs::create_dir_all(source.join("nested")).unwrap();
        std::fs::write(source.join("nested/frame.png"), b"pixels").unwrap();
        let saved = retain_folder(&source, &root.join("documents")).unwrap();
        std::fs::remove_dir_all(&source).unwrap();
        assert_eq!(std::fs::read(saved.join("nested/frame.png")).unwrap(), b"pixels");
        #[cfg(unix)]
        {
            std::fs::create_dir_all(&source).unwrap();
            std::os::unix::fs::symlink(&saved, source.join("outside")).unwrap();
            assert!(retain_folder(&source, &root.join("documents")).unwrap_err().contains("symbolic"));
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn save_names_cannot_escape_documents() {
        for name in ["", "..", ".", "../x", "/tmp/x", "a/b", "a\\b"] {
            assert!(save_path(Path::new("/docs"), name, "ecproj").is_err(), "{name}");
        }
        assert_eq!(save_path(Path::new("/docs"), "My project", "ecproj").unwrap(), PathBuf::from("/docs/My project.ecproj"));
    }
}

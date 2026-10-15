# iOS 27 window bridge: authorized exception and validation

On 2026-10-14 the user requested installation on their iPhone 17e and asked to handle the
previously presented bridge exception. The exception is limited to `attach_window_scene` in
the iOS frontend. It has been implemented and the current-SDK simulator smoke test passes
startup, Metal presentation, display settings, Documents save/open, and playback. A signed
ARM64 device build also passes Xcode bundling and code-signature verification. Physical-device
installation initially hit the free provisioning account's three-app limit. After the user
freed a slot and trusted the certificate, the app installed and launched on their iPhone 17e
(iOS 27.0.1); a screenshot confirmed the editor and intro composition, and the session log
confirmed the Apple A19 Metal compositor.

The engine, shared editor, mobile shell and Xcode target compile. Simulator testing demonstrates
that the iOS 27 scene manifest prevents UIKit's startup assertion, but winit 0.30's editor window
is not attached to the scene. `UIApplication.windows` returns zero windows, so the safe UIKit
enumeration cannot recover it. The app stays alive but shows a black window and does not draw.

The fix is a single, bounded native-handle conversion in `apps/effectcraft-ios/src/mobile.rs`:

```rust
fn attach_window_scene(window: &winit::window::Window) -> Result<(), String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use objc2_ui_kit::{UIApplication, UIView, UIWindowScene};
    let main = objc2::MainThreadMarker::new().ok_or("UIKit needs the main thread")?;
    let app = UIApplication::sharedApplication(main);
    let scene = app.connectedScenes().iter()
        .find_map(|scene| scene.downcast::<UIWindowScene>().ok())
        .ok_or("UIKit has not connected the window scene")?;
    let handle = window.window_handle().map_err(|error| error.to_string())?;
    let RawWindowHandle::UiKit(handle) = handle.as_raw() else {
        return Err("Expected a UIKit window handle".into());
    };
    // SAFETY: winit owns and retains this UIView. The borrowed Window and main-thread
    // marker remain alive throughout this operation; no reference escapes this function.
    let view = unsafe { handle.ui_view.cast::<UIView>().as_ref() };
    let native = view.window().ok_or("The editor view has no UIKit window")?;
    native.setWindowScene(Some(&scene));
    native.makeKeyAndVisible();
    Ok(())
}
```

This is an explicit exception to AGENTS.md's no-unsafe rule. The exception applies
only to this function in the iOS frontend: its crate denies unsafe by default, with a local
allow for this operation; all other workspace crates retain `unsafe_code = "forbid"`. It adds no
Swift, Objective-C implementation, codec code, third-party artwork or unlicensed dependency.

`packaging/ios/smoke.py` is the regression test: launch on a current-SDK simulator, require UI-thread
responses, open display settings, save and reopen an original small composition, and play/stop it. It
failed at startup responsiveness before the fix, reproducing the missing-window integration;
it now passes.

An upstream window backend exposing a safe scene-attachment API would remove this exception.
The raw handle in raw-window-handle 0.6 is a UIView (not a UIWindow); the conversion borrows only
that view, then obtains its independently retained window through UIKit's safe `UIView.window`
method. Neither a borrowed pointer nor a Rust reference is retained by the frontend.

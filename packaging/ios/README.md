# EffectCraft for iPhone and iPad

The native iOS target is `apps/effectcraft-ios`. It runs the shared Rust engine, media importer,
expressions, scripting, render queue, and egui editor through eframe/winit's UIKit backend and
wgpu/Metal. No WebView or Swift bridge is used. One user-authorized native UIView conversion
attaches winit's editor window to UIKit's scene; all other frontend code denies unsafe and
the other workspace crates retain their existing prohibition.

**Current status:** device and simulator compile checks, simulator Xcode bundling, Clippy,
25 simulator unit tests, and the iOS 27.1 end-to-end smoke test pass. The test drives startup,
Metal presentation, safe navigation, Command shortcuts, Documents save/open, playback, PNG and
H.264 MP4 export. The 16 shared menu/shortcut tests also pass, including save completion,
cancellation and failed-write notifications. The ARM64 device app
also builds and signs successfully. On 2026-10-14 it was installed and launched on an iPhone
17e running iOS 27.0.1; a device screenshot confirmed the shared editor and original intro
composition rendered, and the session log confirmed Metal on the Apple A19 GPU. See the
[window bridge decision](../../docs/ios-window-bridge-proposal.md). This remains an early
development port; a full physical-device touch/performance audit and App Store release requirements
remain separate validation steps. The landscape desktop layout at 60%, the notch/corner-safe workspace, and native Files picker
were also verified on the 17e. After the picker was used, a physical-device screenshot showed
imported video in the shared editor. Its iOS 27.0.1 runtime does not expose Xcode’s Resizable App Management capability,
so interactive system resizing remains unverified on physical hardware. The Duo simulator
passes editor/settings/save/open/playback checks; its resizable display can change geometry,
but this does not by itself move the app into a resizable session.

## Build and run

Requires Xcode with an iOS SDK and Rust 1.95 or newer. This machine has a nightly toolchain;
the Xcode build script defaults to `nightly`. Set `EFFECTCRAFT_RUST_TOOLCHAIN=stable` for a
compatible stable installation.

```sh
rustup +nightly target add aarch64-apple-ios aarch64-apple-ios-sim
open packaging/ios/EffectCraft.xcodeproj
```

Select the **EffectCraft** scheme and an iPhone/iPad simulator, then Run. For a physical device,
choose your development team in Signing & Capabilities and use a bundle identifier your team
can sign. Device signing credentials are deliberately not stored in the repository. A free
Apple development account permits three installed development apps on a device; installation
fails when those slots are occupied. Free a slot or use a paid development team, without
overwriting an unrelated app's bundle identifier.

Command-line simulator build on Apple Silicon:

```sh
xcodebuild -project packaging/ios/EffectCraft.xcodeproj -scheme EffectCraft \
  -configuration Debug -sdk iphonesimulator \
  -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath target/ios-xcode CODE_SIGNING_ALLOWED=NO build
```

Output: `target/ios-xcode/Build/Products/Debug-iphonesimulator/EffectCraft.app`.
The Rust build is kept in `target/ios`. Intel Mac simulators additionally require the
`x86_64-apple-ios` Rust target. The deployment target is iOS 18.0.

## Editor and files

The iOS editor defaults to landscape with the shared **Small Screen** desktop workspace:
Project / Effect Controls on the left, Composition in the center, Preview on the right,
and Timeline below. The initial UI scale is **60%**, intended for mouse use. Panels keep their
layout when the window changes size; workspace edits and the selected scale persist.

**Preferences ▸ Appearance** contains the UI scale (25–200%) and an **iOS Display** section
for extra edge spacing (0–48 points) and minimum/maximum resizable window dimensions in iOS
points. Changes preview live; OK saves the display values and Cancel restores them. There is
no separate floating display window or corner launcher. Fresh installs and preference resets
use 60%; the old initial 40% value migrates once, preserving other chosen scales.

The background fills the entire window. Menus and tools remain inside UIKit’s safe bounds;
the panels below use more of the opposite corner gutter on a landscape phone while retaining
the notch-side and home-indicator clearance. Asymmetric Duo/window safe areas stay authoritative.
Extra edge spacing defaults to zero. The earlier default 8-point boxed margin migrates to zero,
preserving other custom values.
Window bounds and safe areas come from the active UIKit window; the rendering view follows its
bounds during fold/unfold and resizing. The app opts into resizing (`UIRequiresFullScreen` is
false), including Duo, iPhone Mirroring and iPad. See Apple’s
[preparation guidance](https://developer.apple.com/iphone-duo/prepare/). UIKit owns the actual outer geometry and can
constrain the configured dimensions; window limits apply only where the scene exposes sizing
restrictions. A fullscreen 17e stays at its system display size.

**File ▸ Open Project**, media import, preset/script import and footage folder selection use
UIKit’s native Files picker, including iCloud Drive and installed Files providers. Media import
supports multiple selections (up to 64). Selected files are streamed into unique
`Documents/Imported/<import-id>/<filename>` folders before the engine reads them. Folder access
uses security-scoped URLs while a worker copies a bounded snapshot, preserving nested paths;
limits are 16 levels, 4,096 entries and 32 GiB. Symlinks are rejected. Existing media is never
overwritten, cancellation leaves the project untouched, and copy/full-disk failures appear as errors.
Live watch-folder monitoring is unsupported on iOS; use Import instead.

Save, Save As, frame/preset/template exports and render output selection use native **Save to Files**.
The engine first writes a durable, unique local copy in `Documents/Saved`; the system picker opens
only after a successful write, or after a queued render finishes. Canceling the picker retains that
local copy. You can choose a provider, rename the export, and confirm replacement through Files.
Command-S opens this destination flow on each save rather than writing back silently to an external
provider. Default render outputs use the writable `Documents/Exports` folder. Image-sequence queue
exports offer the completed staged folder. The MIT-licensed `cidre` and `robius-file-picker` wrappers
provide native APIs without adding another explicit unsafe block to EffectCraft.

Projects and media in Documents are also exposed through Files and Finder file sharing. Opening an
external project keeps a local working copy; projects containing old absolute footage paths may
need **Replace Footage** or **Relink Missing Footage**, which also use native Files. Preferences-only directory/project/model Choose buttons still rely on desktop picker hooks and
need asynchronous iOS integration. Photos import and a share sheet remain separate work.

Hardware keyboards use macOS shortcut conventions: **⌘S**, **⌘O**, **⌘Z**, **⇧⌘Z**, **⌘M**, and
Option modifiers. GameController keyboard transitions supply Command/Control separately, because
winit’s UIKit text events omit those modifiers. UIKit still supplies text and IME input; text-field
copy, cut and paste use the system clipboard. App shortcuts pause while a native picker is open.
Physical shortcut key mapping currently follows US HID positions; localized text remains UIKit’s.

Settings and recovery data use `Library/Application Support/EffectCraft`; cache entries use
`Library/Caches/EffectCraft/Disk Cache`. Startup offers existing crash recovery. Losing focus stops
preview and writes a synchronous auto-save when the project is dirty; background/suspension behavior
still needs physical-device validation. The cpal adapter supplies native audio preview.

## Mobile memory policy

- Effective layer and decoded-footage caches: at most 128 MiB each.
- Effective finished-preview cache: at most 256 MiB.
- Two native frame workers; no default idle caching.
- Retained GPU viewer frames: initially at most 128 MiB.
- Persistent disk cache: initially at most 2 GiB.

RAM ceilings remain effective when loading desktop preferences or changing cache settings. These
are cache budgets, not a process-wide memory limit: decoder state, active render buffers, imported
models and disk-write staging can exceed them. Large compositions need a future shared allocation
budget and UIKit memory-warning integration. Neither physical-device performance nor App Store
readiness is claimed by this initial port. App icons, a share sheet, memory warnings,
and a complete touch/accessibility review remain release work.

## Validation

```sh
cargo +nightly check -p effectcraft-ios --target aarch64-apple-ios-sim --locked
cargo +nightly check -p effectcraft-ios --target aarch64-apple-ios --locked
cargo +nightly clippy -p effectcraft-ios --target aarch64-apple-ios-sim --all-targets -- -D warnings
cargo +nightly test -p effectcraft-ios
cargo +nightly xtask layers
cargo +nightly xtask assets
```

For the current-SDK launch regression, boot a simulator and run:

```sh
python3 packaging/ios/smoke.py DEVICE_UUID
# A separate simulator device set (useful when the default device folder is inaccessible):
python3 packaging/ios/smoke.py DEVICE_UUID --set /tmp/effectcraft-simulators
```

Debug builds accept `--control=PORT`, `--demo`, and `--import` (opens the native picker after the first editor frame). The opt-in localhost control server is the
desktop protocol and is compiled out of Release builds. The smoke test creates an original
small project and saves it with a unique `Documents/iOS Smoke-*.ecproj` name; it does not modify
existing projects. Cold simulator Metal pipeline compilation can take about a minute, so the
startup wait allows up to two minutes.

The mobile shell also builds on macOS for headless UI tests and development. This is separate
from the shipping desktop frontend. Workspace rules and the full `cargo xtask ci` gate still apply
before committing. The UIKit event loop uses `run_and_return = false`, as required by the
[winit iOS backend](https://docs.rs/winit/0.30.13/winit/platform/ios/index.html).

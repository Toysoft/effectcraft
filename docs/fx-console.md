# FX Console

> **Last reviewed:** 2026-10-11 · **Last updated:** 2026-10-11 · **Change:** major (first version) · **Target:** Adobe After Effects 2026 with Video Copilot's FX Console 1.0.7

FX Console is a keyboard-first launcher for effects and animation presets, built into EffectCraft
(Edit ▸ FX Console…, **Ctrl+Space**). Its behaviour follows Video Copilot's free FX Console
plug-in for After Effects as its public [blog post](https://www.videocopilot.net/blog/2018/05/fx-console-updated-to-v1-0-3/)
and [tutorial video](https://youtu.be/rqmDo04T0pY) show it; no code, artwork or presets of the
plug-in are used. Everything it does is an engine command (`fxConsole.*`), so agents, scripts and
the CLI get the same search and the same settings as the console.

## Apply effects from the keyboard

Select one or more layers and press **Ctrl+Space** (Control+Space on macOS, where Cmd+Space is
Spotlight; change it in Edit ▸ Keyboard Shortcuts or FX Console Settings ▸ General). A search box
opens under the pointer.

| Key | What it does |
|---|---|
| typing | Searches effects and animation presets: part of a name, its initials (`gb` → Gaussian Blur), its letters run together (`4color` → 4-Color Gradient) or four or more of its letters in order (`frctl` → Fractal) |
| ↑ ↓, Page Up / Page Down | Pick a row |
| Enter | Apply it to the selected layers and close |
| Shift+Enter | Apply it and keep the console open, to add several effects in a row |
| Tab / Shift+Tab | Jump to the Presets section and back |
| 1–9 (empty search) | Apply shortcut 1–9 at once |
| Escape, a click outside | Close |

Results come in two sections, **Effects** (blue) and **Presets** (red: EffectCraft's text
animation presets and your `.ecpreset` files from `~/Documents/EffectCraft/Presets`, or
`$EC_PRESETS_DIR`). The typed text is highlighted; a ★ marks favorites and a number their
shortcut. With an empty search the console lists the shortcuts and the favorites.

The toolbar takes a **snapshot** of the composition into the gallery, opens the **gallery**,
**copies** the frame to the clipboard or **saves** it as PNG or JPG, and opens the **settings**.

## Teach it how you work

Right-click a result:

- **Add to Favorites**: favorites come first whenever they match, in the order you give them in
  FX Console Settings ▸ Favorites (their priority). Type `fractal` and Fractal Noise, a favorite,
  comes before Fractal.
- **Alias and Override…**: an **alias** is another name a favorite answers to (comma-separated:
  `corner pin, cp` makes CC Power Pin come first when you type "corner pin"). An **override** is
  an animation preset applied in the favorite's place: the effect with your own defaults, or
  several effects with expressions. Set an effect up in Effect Controls, select it and choose
  *Save Selected Effects as Override* (or pick any `.ecpreset` with Animation ▸ Save Animation
  Preset… first). If the preset file goes missing, the effect itself is applied, with a warning.
- **Blacklist**: hide an effect or preset you never use (Corner Pin once you prefer CC Power Pin).
  Settings ▸ Blacklist shows them again.
- **Create Shortcut / Replace Shortcut**: nine numbered shortcuts with your own labels, applied
  with 1–9 in an empty console or with a click in the FX Console panel. Settings ▸ Shortcuts
  reorders and relabels them.

FX Console Settings ▸ General sets the console's colour, its shortcut key, whether presets are
listed, how many rows each section shows, the snapshot format (PNG keeps transparency, or JPG with
a quality) and the gallery folder, and imports or exports the settings as a JSON file (merge or
replace). They are kept in the settings store as `fx_console.json`, never in the project.

## Snapshots and the gallery

A snapshot is the current frame at the viewer's resolution, with transparency when the viewer
shows the transparency grid. Snapshots go to the gallery folder (by default `FX Console Gallery`
next to the settings): a scratch place for comparing design ideas, not an archive.

Window ▸ **FX Console Gallery** is a floating window:

- click a snapshot, ↑ ↓ to step through them; Ctrl/Cmd- or Shift-click (or Shift+↑ ↓) to compare
  up to four side by side; **Z** shows 100 %;
- **Copy** (Ctrl/Cmd+C), **Import into Project**, **Export…** (converting PNG ↔ JPG),
  **Delete** (Delete), **Reveal** in the file manager; or drag a snapshot into the Project panel
  or the Timeline to import it;
- while an Effect Controls eyedropper is armed, a click on a snapshot samples its real colour, so a
  look can be matched across projects.

Window ▸ **FX Console** is a dockable panel with the console's tools and the nine shortcuts.

## For agents

| Command | |
|---|---|
| `fxConsole.search {query?, limit?}` | The ranked results (`effects`, `presets`); an empty query gives `shortcuts` and `favorites` |
| `fxConsole.apply {effect? \| textPreset? \| preset? \| query? \| slot?, layers?}` | Apply as the console does (overrides included) |
| `fxConsole.favorite`, `fxConsole.alias`, `fxConsole.override`, `fxConsole.blacklist`, `fxConsole.shortcut` | Teach it (targets as `effect` id or name, `textPreset` or `preset` path) |
| `fxConsole.settings`, `fxConsole.setSettings`, `fxConsole.exportSettings`, `fxConsole.importSettings` | Options and settings files |
| `fxConsole.snapshot`, `fxConsole.saveFrame`, `fxConsole.gallery`, `fxConsole.deleteSnapshots`, `fxConsole.exportSnapshots`, `fxConsole.importSnapshots`, `fxConsole.revealGallery` | Snapshots and the gallery (snapshot names are plain file names inside the gallery) |
| `app.fxConsole {query?}`, `fxConsole.openGallery`, `fxConsole.openSettings {page?}`, `fxConsole.copyFrame` | The frontend's console, windows and clipboard |

With `--automation-read-root` / `--automation-write-root`, settings files and saved frames must be
inside the roots, and the commands that reach files the settings name (shortcut and override
presets, the gallery) are refused.

The code: `crates/engine/src/fx_console/` (settings, search, gallery),
`crates/engine/src/commands/fx_console.rs` (commands) and
`crates/ui-egui/src/panels/fx_console.rs` (the console, its settings, the gallery and the panel).

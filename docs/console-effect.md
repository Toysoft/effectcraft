# Console Effect

> **Last reviewed:** 2026-10-11 · **Last updated:** 2026-10-11 · **Change:** major (first version) · **Target:** Adobe After Effects 2026

Console Effect is a keyboard-first launcher for effects and animation presets (Edit ▸ Console
Effect…, **Ctrl+Space**). It is the kind of tool After Effects should have natively, as part of
the application rather than an add-on, so in EffectCraft it is built in and on by default. It is
based on FX Console. Everything it does is an engine command (`consoleEffect.*`), so agents,
scripts and the CLI get the same search and the same settings as the console.

## Apply effects from the keyboard

Select one or more layers and press **Ctrl+Space** (Control+Space on macOS, where Cmd+Space is
Spotlight; change it in Edit ▸ Keyboard Shortcuts or Console Effect Settings ▸ General). A search box
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
  Console Effect Settings ▸ Favorites (their priority). Type `fractal` and Fractal Noise, a favorite,
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
  with 1–9 in an empty console or with a click in the Console Effect panel. Settings ▸ Shortcuts
  reorders and relabels them.

Console Effect Settings ▸ General sets the console's colour, its shortcut key, whether presets are
listed, how many rows each section shows, the snapshot format (PNG keeps transparency, or JPG with
a quality) and the gallery folder, and imports or exports the settings as a JSON file (merge or
replace). They are kept in the settings store as `console_effect.json`, never in the project.

## Snapshots and the gallery

A snapshot is the current frame at the viewer's resolution, with transparency when the viewer
shows the transparency grid. Snapshots go to the gallery folder (by default `Console Effect Gallery`
next to the settings): a scratch place for comparing design ideas, not an archive.

Window ▸ **Console Effect Gallery** is a floating window:

- click a snapshot, ↑ ↓ to step through them; Ctrl/Cmd- or Shift-click (or Shift+↑ ↓) to compare
  up to four side by side; **Z** shows 100 %;
- **Copy** (Ctrl/Cmd+C), **Import into Project**, **Export…** (converting PNG ↔ JPG),
  **Delete** (Delete), **Reveal** in the file manager; or drag a snapshot into the Project panel
  or the Timeline to import it;
- while an Effect Controls eyedropper is armed, a click on a snapshot samples its real colour, so a
  look can be matched across projects.

Window ▸ **Console Effect** is a dockable panel with the console's tools and the nine shortcuts.

## For agents

| Command | |
|---|---|
| `consoleEffect.search {query?, limit?}` | The ranked results (`effects`, `presets`); an empty query gives `shortcuts` and `favorites` |
| `consoleEffect.apply {effect? \| textPreset? \| preset? \| query? \| slot?, layers?}` | Apply as the console does (overrides included) |
| `consoleEffect.favorite`, `consoleEffect.alias`, `consoleEffect.override`, `consoleEffect.blacklist`, `consoleEffect.shortcut` | Teach it (targets as `effect` id or name, `textPreset` or `preset` path) |
| `consoleEffect.settings`, `consoleEffect.setSettings`, `consoleEffect.exportSettings`, `consoleEffect.importSettings` | Options and settings files |
| `consoleEffect.snapshot`, `consoleEffect.saveFrame`, `consoleEffect.gallery`, `consoleEffect.deleteSnapshots`, `consoleEffect.exportSnapshots`, `consoleEffect.importSnapshots`, `consoleEffect.revealGallery` | Snapshots and the gallery (snapshot names are plain file names inside the gallery) |
| `app.consoleEffect {query?}`, `consoleEffect.openGallery`, `consoleEffect.openSettings {page?}`, `consoleEffect.copyFrame` | The frontend's console, windows and clipboard |

With `--automation-read-root` / `--automation-write-root`, settings files and saved frames must be
inside the roots, and the commands that reach files the settings name (shortcut and override
presets, the gallery) are refused.

The code: `crates/engine/src/console_effect/` (settings, search, gallery),
`crates/engine/src/commands/console_effect.rs` (commands) and
`crates/ui-egui/src/panels/console_effect.rs` (the console, its settings, the gallery and the panel).

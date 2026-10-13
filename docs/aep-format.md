# After Effects project format (`.aep`)

> **Last reviewed:** 2026-10-13 · **Last updated:** 2026-10-13 · **Change:** second round of verified field layouts (keyframe sides and ease, record templates, anchor divisor, expression switch, layer switches, blend modes, NTSC time base, mask modes, shape layers, text runs, marker strings, camera, light, effect values) added to the typed view of `effectcraft-aep` · **Target:** Adobe After Effects 2026

What `crates/aep` ([`effectcraft-aep`](../crates/aep/README.md)) reads and writes, and the evidence
for each statement. Everything here was observed on project files we generated ourselves; nothing
comes from a third-party parser, specification or source. This is the container and a set of
verified field layouts, not a project importer: turning a parsed `.aep` into an EffectCraft
document is future work (see [gaps.md](gaps.md) G4 and [file-format-parity.md](file-format-parity.md)).

Evidence is cited as **case numbers**: the files `NNN-*.aep` of our synthetic corpus, each made by
one scripted change to a base project. Batch 1 (cases 000 to 126) was made in one chain of
Save As; batch 2 (cases 200 to 259) was made in later sessions, each case starting again from the
same loaded base. The base project `000-base` is one composition `C` (1920x1080, 25 fps, 5 s)
holding one red solid layer `L` (1920x1080) in the folder `Solids`. 103 of these files are the
test fixtures in `crates/aep/tests/fixtures/` (7.8 MB together; the small ones of each group, the
rest are named in section 4); the tests named below run on them. The container of every fixture is
byte-identical to the generated file, with two exceptions that keep the fixtures small and free of
local data: the XMP packet after the container (which grows by 1.7 KB with every save of the corpus
chain) is replaced by the 5.8 KB packet of the base project in all fixtures but `001`, and in `126`
the file path and machine name inside the footage alias JSON are replaced by neutral text of the
same length.

Confidence codes in the field tables: **V** = a test in `crates/aep/tests/` asserts that the decoded
value equals the value the case was made with (the corpus manifest); **E** = decoded and consistent
in the case diffs, but no manifest value to assert against (the field is exposed only where a test
pins what the fixtures contain); fields that the corpus does not decide (**?**) or only suggests
(**H**) are not decoded; they are listed in section 4.

## 1. Container

| Fact | Evidence |
|---|---|
| The file starts with `RIFX`, a big-endian `u32` size and the form type `Egg!`; all integers in the structure are big-endian (the one exception, `iide`/`idpi`, is little-endian). | All 139 files of batches 1 and 2. |
| The declared size is `4 + the chunks`; it ends before the end of the file. The bytes after it are an XMP packet (`<?xpacket begin` ... `<?xpacket end=...?>`, UTF-8 XML, 5.8 KB in `000`) that grows with every save. | All files; test `header_and_trailer_facts`. |
| A chunk is a 4-byte id, a big-endian `u32` body size and the body; an odd-sized body is followed by one pad byte, always `0` in the corpus (15 622 odd bodies). | Test `odd_sized_chunks_keep_their_pad_byte_even_when_not_zero` keeps any value. |
| The chunks of every file tile the container exactly: no leftover bytes, no chunk larger than its parent. | 139 of 139 files parse without a repair and write back byte for byte. |
| A `LIST` chunk's body is a 4-byte list type followed by chunks. List types seen include `Fold`, `Item`, `Layr`, `tdgp`, `tdbs`, `list`, `Pin ` and `FEE ` (the space is part of the type). | `000`; test `list_types_ending_in_a_space_are_kept`. |
| Lists can be empty (size 4, only the type): `Ewst`, `Pefl`, `CPPl`, `LItm`. | `000`; test `empty_list_and_list_shorter_than_its_type`. |
| Nesting: at most 13 levels in the corpus (the path of a shape layer, 240). | All files; test `nesting_depth_is_below_the_limit`. |
| `LIST btdk` is not a chunk sequence: its body (7775 bytes in `110`) is text. A reader must keep it opaque. Only files with a text layer have it. | `110`-`114`, `243`-`248`; test `raw_text_lists_are_kept_opaque`. |
| Three non-`LIST` ids hold a chunk instead of data: `tdsn`, `pdnm`, `fnam`. Each holds exactly one `Utf8` chunk (a name or label, UTF-8 text, possibly empty). | `tdsn` 31 209 times, `fnam` 40, `pdnm` 32 over the corpus; test `nested_chunk_ids_hold_one_utf8_chunk`. |
| Ids can end in a space; the space is part of the id. In the corpus this happens for the list types `Pin ` and `FEE `. All ids are 4 printable ASCII bytes. | `Pin `, `FEE ` in all files but `001`; tests `list_types_ending_in_a_space_are_kept`, `ids_ending_in_a_space_are_kept`. |
| Payloads are plain: no compression, no encryption. Text is readable, numbers are IEEE big-endian doubles or integers. | `010` (the layer name `Renamed` is readable in a `Utf8` chunk), `040` (`100.0` and `200.0` as doubles). |

### How the crate handles it

- `parse` builds a tree of `Chunk { id, body, pad }`; a body is `Data`, `Chunks` (the three
  nested ids), `List { list_type, children }` or `OpaqueList { list_type, data }`. The bytes after
  the container are kept as the trailer.
- `write` recomputes every size from the tree. For every input that parses, `write(parse(x)) == x`
  byte for byte, including pad bytes, an odd last body without a pad byte, and the trailer. The
  tests assert this for all fixtures and for every damaged input that still parses.
- A `LIST` (or nested id) whose body is not a well-formed chunk sequence, or that is nested deeper
  than `Limits::max_depth` (64), stays opaque instead of failing the parse. At the top level, a
  chunk that does not fit, a wrong form type or a container larger than the input is an error.
  Sizes are checked against the remaining input; nothing is allocated from a declared size; the
  chunk count is bounded (`Limits::max_chunks`).

## 2. Typed view (`Project`, `Item`, `Comp`, `Layer`, `PropGroup`, `Property`, `KeyList`)

The structure the typed view is built on; each row has a test in `crates/aep/tests/typed.rs` against
the values the cases were made with. The field layouts that build on it (layer and composition
headers, values with their units, keyframes, effects, masks, shapes, text, markers, cameras,
lights, footage) are in section 3. Everything else is reachable as raw chunks.

| Fact | Evidence |
|---|---|
| Project items are `LIST Item` children of the top-level `LIST Fold`; a folder holds its children in a nested `LIST Sfdr`. | `000`: composition `C` and folder `Solids` at the top, the solid's footage item inside `Solids`. |
| The item kind is the first big-endian `u16` of `idta`: 1 folder, 4 composition, 7 footage. | `000` (`C`, `Solids`, the solid `L`); `001` has no composition. |
| The item name is the item's `Utf8` child (empty for a solid's footage item). | `000` (`C`, `Solids`), `036` (comp renamed: the `Utf8` grows from 1 to 12 bytes, `Renamed Comp`). |
| A composition item has a `cdta` whose `u32` at byte 4 is the frame duration and the `u32` at byte 8 the timescale: `fps = timescale / frame_duration` (25 fps: 1024 and 25 600; 29.97 fps: 800 and 23 976). Keyframe times are in these units. | `000`, `031` (30 fps: 1024, 30 720), `229`, `230`; `055` (key at 2.5 s = 64 000 units). |
| The layers of a composition are its `LIST Layr` children, top layer first. (The `DLay`, `SLay`, `CLay` and `SecL` lists next to them are not layers: their `ldta` names are `Default`, `Front`, `Left`, `Top`, `Back`, `Right`, `Bottom`, `Custom View 1` to `3` and `Markers`.) | `000` (one layer), `110` (text layer on top of `L`). |
| The layer name is the `Utf8` child of the layer; it is empty while the layer shows its source's name (new solids, precomp layers, default-named cameras and lights). | `010` (`L` renamed: 0 to 7 bytes, `Renamed`), `110` (`T`), `211` (`S100` is the solid's name, not the layer's). |
| The 3D layer switch is bit `0x04` of byte 38 of `ldta`. | `015` (only that byte changes from `00` to `04`; the 3D properties appear), `000`. |
| A layer holds its property tree in a `LIST tdgp`. Inside a group, a `tdmn` chunk (the match name: ASCII, NUL padded to 40 bytes) precedes the list it names: `tdgp` (a group), `tdbs` (a property) or another list (`otst`, `om-s`, the effect instance `sspc`). A `tdmn` named `ADBE Group End` closes the group and names nothing. | `000` (`ADBE Transform Group`), `071` (`ADBE Effect Parade`, `ADBE Slider Control`), `240`. |
| Properties are stored sparsely: a property at its default has no `tdbs` at all (the default depends on the layer and the composition). | `040`, `041`, `043`, `044`: each adds exactly one `tdmn` and one `tdbs` to `000`; `211`, `212` (new solids: Position absent, Anchor present only when changed); test `static_value_is_the_first_dims_doubles_of_cdat`. |
| A property (`tdbs`) holds `tdsb`, `tdsn`, the 124-byte header `tdb4`, then the static value `cdat` or a keyframe `LIST list`. `tdb4` begins `db 99 00`; byte 3 is the number of components (Position 3, Scale 3, Opacity 1, Slider 1, colour 4); byte 68 is 1 exactly when the property has a `list`. | `040`, `041`, `043`; `050` (the `cdat` is replaced by a `list`, byte 68 goes 0 to 1); `055`; test `property_header_facts_hold_in_every_property_of_every_fixture`. |
| The static value is the first `dims` big-endian doubles of `cdat`. Percent properties are stored as fractions: Scale 50 % is `0.5, 0.5, 1.0`, Opacity 25 % is `0.25`. | `040` (Position `100, 200, 0`), `041`, `043`, `071` (Slider `77`). |
| A `LIST list` is `lhd3` (52 bytes, 13 `u32`) and `ldat`. Word 0 is `0x00d00bee`, word 2 the record count, word 4 the record size, words 8 to 12 are zero; `len(ldat) = count * record size`; a list without records has no `ldat`. In the keyframe lists of properties, word 3 is `ceil(count / 4)` and word 7 the count rounded up to a multiple of 4 (not in the empty `Gide` lists, and not in path lists, where they are allocation capacities). | The first four relations hold for every `list` of every fixture (test `list_header_facts_hold_in_every_list_of_every_fixture`); word 3 and 7 on the keyframe lists of `050` (1 record) and `055` (3 records of 128 bytes, capacity 4). |
| A keyframe record starts with its time as a big-endian `i32` in the composition's time units (**layer time**). | `055` (0, 25 600, 64 000 for keys at 0 s, 1 s, 2.5 s), `058` (0 and 51 200 for keys at 0 s and 2 s). |

## 3. Verified field layouts

All integers are big-endian unless stated; offsets are byte offsets inside the chunk body; `f64`
is an IEEE-754 double, `f32` a float. Chunk paths start at the project item; `Layr` is a real layer
(`LIST Layr`), `tdgp[X]` is the `LIST tdgp` named `X` by the `tdmn` that precedes it. Each row lists
the cases that decide it. A row marked V is asserted by a test that compares the decoded value with
the value from the corpus manifest (test file in parentheses at the end of each table).

### 3.1 Layer header (`Layr/ldta`, 164 bytes; the decoder needs the first 140)

| Field | Offset | Type | Meaning | Evidence | Conf. |
|---|---|---|---|---|---|
| id | 0 | u32 | layer id, unique in the project; parent links use it | 000 (`0x1b`), 022 | V |
| quality | 5 | u8 | 2 = Best, 1 = Draft (other values are kept as `Other`; 0 on cameras and lights) | 000, 023 | V |
| stretch | 8 | u32 | time stretch as a whole multiplier: 1 = 100 %, 2 = 200 % | 000, 018 | V (1, 2) |
| start | 12 / 16 | i32 / u32 | layer start time = num / den seconds; den = composition timescale (25 600), 1 in a layer made by the precomp operation | 000, 021 (12 800 / 25 600 = 0.5 s), 124 | V |
| in point | 20 / 24 | i32 / u32 | **relative to the start**: absolute = start + in * stretch | 000, 019 (25 600 / 25 600 = 1 s), 021 (stays 0 while the layer starts at 0.5 s) | V |
| out point | 28 / 32 | i32 / u32 | same convention; 5 s = 128 000 / 25 600 | 000, 018 (stays 5 s, the layer ends at 10 s), 019 (6 s), 021 (5.5 s) | V |
| name is user-set | 37 bit `0x01` | flag | clear while the layer shows its source's name (000, 124, new solids, default-named camera and light), set when named | 000, 010, 022, 110, 113, 124, 211 | V |
| guide layer | 37 bit `0x02` | flag | | 026 | V |
| adjustment layer | 38 bit `0x02` | flag | | 025 | V |
| 3D layer | 38 bit `0x04` | flag | always set on cameras and lights | 015, 251 | V |
| solo | 38 bit `0x08` | flag | | 014 | V |
| auto-orient | 38 bit `0x40` | flag | orients towards the point of interest: set on two-node cameras (byte 38 is `0x44`) and on all lights, clear on a one-node camera (`0x04`) | 256 (`0x44`), 251 (`0x04`), 252, 254 | V |
| null object | 38 bit `0x80` | flag | set on a layer created as a null | 022, 220 | V |
| **video switch** | 39 bit `0x01` | flag | the eye icon (`enabled`); off gives byte 39 = `06` | 222 | V |
| **audio switch** | 39 bit `0x02` | flag | `audioEnabled` (also changes on a solid); off gives `05` | 223 | V |
| **effects switch** | 39 bit `0x04` | flag | `effectsActive`; off gives `03` | 224 | V |
| motion blur | 39 bit `0x08` | flag | | 017 | V |
| locked | 39 bit `0x20` | flag | | 013 | V |
| shy | 39 bit `0x40` | flag | | 012 | V |
| collapse transformations | 39 bit `0x80` | flag | continuously rasterize; set from creation on shape and text layers | 024, 238 | V |
| source id | 40 | u32 | `idta@16` of the source item (solid, footage or comp); 0 for shape, text and camera layers, `0xffff_ffff` for lights | 000, 022, 124, 126, 252 | V |
| label | 61 | u8 | label colour index (1 red, 5, 8 blue, 15, ...) | 000 (1), 124 (15), 126 (5) | V |
| name copy | 64..95 | ASCII, NUL padded | copy of the user-set name; empty while the layer is unnamed | 000 (empty), 010 (`Renamed`) | V |
| **blend mode** | 99 | u8 | Normal 2, Add 4, Multiply 5, Screen 6, Overlay 7, Difference 26; any other code is kept raw. Each case changes only this byte | 000, 016, 225-228 | V (6 modes) |
| layer type | 128 | u32 | 0 AV (solid, footage, precomp, null), 1 light, 2 camera, 3 text, 4 shape | AV: 000, 022, 126; text: 110, 113, 243; shape: 238-242; camera: 251, 253, 256; light: 252, 254 | V |
| parent id | 132 | u32 | 0 = no parent | 000, 022 (L gets the id of the new null layer) | V |
| **light type** | 136 | u32 | 1 = spot, 2 = point (0 on every other layer); parallel and ambient were not sampled and are kept raw | 252, 254 | V |

API: `LayerHeader` (`is_video_enabled`/`is_enabled`, `is_audio_enabled`, `is_effects_enabled`,
`is_auto_oriented`, `light_type`, `blend_mode`); the same switches on `Layer`. Tests:
`tests/header.rs`, `tests/layer_switches.rs`. The layer name itself is the `Utf8` child of the
`Layr` (empty while the name is the source's); the layer then shows the name of its source item
(`Project::layer_display_name`).

### 3.2 Composition header (`Item/cdta`, 204 bytes; the decoder needs the first 176) and renderer

| Field | Offset | Type | Meaning | Evidence | Conf. |
|---|---|---|---|---|---|
| frame duration | 4 | u32 | time units per frame: 1024 at 25 and 30 fps, **800 at 29.97 fps, 1000 at 23.976 fps** | 000, 031, 229, 230 | V |
| timescale | 8 | u32 | time units per second: 25 600 at 25 fps, 30 720 at 30 fps, **23 976 at 29.97 and 23.976 fps**; fps = timescale / frame duration exactly as typed (29.97 is `23976 / 800`, not `30000 / 1001`) | 000, 031, 229, 230 | V |
| work area start | 28 | u32 | in time units | 035 (25 600 = 1 s) | V |
| work area end | 36 | u32 | start + duration in time units; `0xffff_ffff` = the whole composition | 000, 035 (76 800 = 3 s) | V |
| duration | 44 | u32 | in time units. **A duration is snapped to whole frames: 5 s at 29.97 fps is 120 000 units = 150 frames = 5.005005 s, at 23.976 fps 120 frames** | 000, 031, 032 (256 000), 229, 230 | V |
| background | 52..54 | u8 R, G, B | **8 bits per channel**: 0.5 is stored as 128 and reads back as 128/255 = 0.50196 | 000, 033, 231 | V |
| motion blur | 139 bit `0x08` | flag | the composition's switch | 038 | V |
| width, height | 140, 142 | u16, u16 | | 000, 030 (1280 x 720) | V |
| pixel aspect | 144 / 148 | u32 / u32 | numerator / denominator | 000 (1 / 1), 037 (2 / 1) | V |
| frame rate | 156 | u32 | 16.16 fixed point of the float32 rate (25 = `0x0019_0000`, 29.97 = 1 964 114 = 29.9700012, 23.976 = 1 571 291) | 000, 031, 229, 230 | V |
| shutter angle | 172 | u32 | degrees | 000 (180), 034 (90) | V |
| renderer | `LIST PRin` / `prin` 4 | 32 ASCII | match name: `ADBE Escher` in the base, `ADBE Calder` in 039 | 000, 039 | V |
| renderer name | `prin` 52 | 32 ASCII | display name: `Classic 3D`, `Advanced 3D` | 000, 039 | V |
| composition name | `Item/Utf8` | text | | 000 (`C`), 124 (`Pre`) | V |

Every property's keyframe time scale (`tdb4` bytes 12 to 15) follows the composition (23 976 in every
property of `L` after 229 and 230). The corpus manifest calls the classic renderer
`ADBE Advanced 3d`; the file stores `ADBE Escher` / `Classic 3D`. API: `CompHeader::frame_rate_exact`
(exact fraction), `duration_frames`. Tests: `tests/header.rs`, `tests/layer_switches.rs`.

### 3.3 Items, footage and precomps

| Field | Chunk path | Offset | Type | Meaning | Evidence | Conf. |
|---|---|---|---|---|---|---|
| item kind | `Item/idta` | 0 | u16 | 1 folder, 4 composition, 7 footage | 000 | V |
| item id | `Item/idta` | 16 | u32 | what a layer's source id refers to (`iide` holds the same number little-endian) | 000 (`0x1a` solid, `0x0d` comp), 022, 124 | V |
| item label | `Item/idta` | 58 | u8 | label colour of the item | 000 (1 solid), 124 (15 for `Pre`) | V |
| used comp | `Item/idpi` | 0 | u32 **little-endian** | a composition that uses another as a precomp layer gets one `idpi` per used composition | 124 (`C` uses `Pre`) | V |
| footage type tag | `Pin /sspc` | 22 | 4 ASCII | `Soli` for a solid, `png!` for a png | 000, 126 | V |
| footage size | `Pin /sspc` | 30, 34 | u32, u32 | width, height in pixels | 000 (1920 x 1080), 022 (100 x 100 null), 126 (8 x 8), 211, 212, 259 | V |
| footage name | `Pin /opti` | 26 (solid), 58 (file) | ASCII, NUL terminated | the item's own `Utf8` name is empty for footage | 000 (`L`), 022 (`Null 1`), 126 (`test8x8.png`), 211 (`S100`), 259 (`Blue`) | V |
| **solid colour** | `Pin /opti` | 10, 14, 18, 22 | 4 x f32 | ARGB, 0..1: red [1, 0, 0] is `1, 1, 0, 0`, blue [0, 0, 1] is `1, 0, 0, 1` | 000, 259 | V |
| file path | `Pin /Als2/alas` | key `fullpath` | JSON string | alias JSON of a file footage item | 126 (the fixture holds a neutral path) | V |
| precomp linkage | `Layr/ldta` | 40 | u32 | the layer's source id is the id of the precomp's item | 124 | V |

Test: `tests/header.rs` (precomp), `tests/text_markers_footage.rs` (footage),
`tests/layer_switches.rs` (solid colour).

### 3.4 Properties: sparse storage, static values and units

A layer's property tree contains a `tdbs` only for a property whose value differs from the default
that After Effects computes for the layer and composition. A reader supplies the defaults:

| Property (match name) | Stored | Unit in the API | Default when absent | Evidence | Conf. |
|---|---|---|---|---|---|
| `ADBE Position` | f64 x, y, z | pixels | the composition centre `[width / 2, height / 2, 0]` | 000, 040 (`100, 200, 0`), 030 (the comp becomes 1280 x 720, L stays 1920 x 1080: `960, 540, 0` is now stored), 211, 212 (absent on new solids) | V |
| `ADBE Anchor Point` | f64 x, y as a **fraction of the layer's source size**, z in pixels | px = fraction * source size | `0.5, 0.5, 0` (the layer centre) | 044 (`10, 20` px on 1920 x 1080 is `10/1920, 20/1080`), **211 (100 x 100 solid: `10, 20` is stored `0.1, 0.2`), 212 (400 x 300 solid: `100, 50` is stored `0.25, 1/6`)**: the composition size is not the divisor | V |
| `ADBE Scale` | f64 fractions | percent (fraction * 100) | 100 % | 000, 041 (`0.5, 0.5, 1.0`) | V |
| `ADBE Rotate Z` | f64 degrees | degrees | 0 | 000, 042 | V |
| `ADBE Opacity` | f64 fraction | percent (fraction * 100) | 100 % | 000, 043 (`0.25`) | V |
| mask feather | f64 x, y | pixels | `[0, 0]` | 090, 091 (`20, 20`), 208 | V |
| mask opacity | f64 fraction | percent | 100 % | 090, 091 (`0.5`) | V |
| mask expansion `ADBE Mask Offset` | f64 | pixels | 0 | 090, 091 (5) | V |
| effect slider | f64 | as set | the `pard` default | 071 (77) | V |
| effect colour | 4 x f64, ARGB 0..255 | ARGB fractions | the `pard` default | 073 (`[0.2, 0.6, 1]` is `255, 51, 153, 255`) | V |
| shape stroke width, round corners radius, light intensity | f64 | as set (pixels, pixels, percent) | not decoded as defaults except the light (100) | 239 (8), 238 (20), 254 (50) | V |
| shape group position, gradient start and end | f64 x, y | pixels | group position `[0, 0]` | 242, 241 | V |
| camera Position, point of interest | f64 x, y, z | pixels | see 3.12 | 251, 253, 256 | V |

`Project::layer_anchor_px` multiplies the anchor fraction by the size of the layer's source (a
solid or footage item, or the composition of a precomp).

Property record (`LIST tdbs`: `tdsb`, `tdsn`, `tdb4`, then `cdat` or a `LIST list`, optionally a
`Utf8` expression, `tdum`, `tduM`):

| Field | Chunk path | Offset | Type | Meaning | Evidence | Conf. |
|---|---|---|---|---|---|---|
| magic | `tdb4` | 0..2 | `db 99 00` | every property | all fixtures | V |
| components | `tdb4` | 3 | u8 | values in `cdat` / in each key (1, 2, 3, 4 = ARGB) | 040, 041, 043, 073, 091, 206, 208, 210 | V |
| static flag | `tdb4` | 5 bit 0 | flag | set while the property has no keys (Position: 15 static; the bit is clear on an animated property) | 040, 055, 058 | V |
| **template flag** | `tdb4` | 7 | u8 | 0 = plain keyframe template, non-zero = spatial template (3 on Position, 1 on colours and paths) | 204-206, 208, 210 | V |
| time scale | `tdb4` | 12 | u32 | units per second of the keyframe times (25 600; 30 720 at 30 fps; 23 976 at 29.97 and 23.976 fps) | 055, 058, 122, 229, 230 | V |
| has keys | `tdb4` | 68 | u8 | 1 exactly when the property has a `LIST list` | every fixture (test) | V |
| expression flags | `tdb4` | 76, 77, 120 | u8 | all three are 1 when an expression exists. 76 and 77 are also 1 on a camera's or light's computed (auto) Position, which has no expression | 060, 213, 251 | V |
| **expression disabled** | `tdb4` | 119 | u8 | 1 = the expression is disabled; the text and bytes 76, 77, 120 stay. On the built-in view layers the byte flips by itself after the first save, so it counts only together with an expression | 060 (0), 213 (1) | V |
| state | `tdsb` | 0 | u32 | 1 = written (non-default value, keys or expression), 3 = default state | 040, 071, 073 (1); 3 on the unwritten properties of 000 | V (1), E (3) |
| static value | `cdat` | 0 | `components` x f64 | the first `components` doubles are the value | 040, 041, 043, 071 | V |
| user name | `tdsn/Utf8` | | text | `-_0_/-` means "no custom name" | 091 (`Mask 1`), 238 (`Group 1`) | V |

Test: `tests/values.rs`, `tests/typed.rs`, `tests/keyframes.rs`.

### 3.5 Keyframes (`tdbs/list`: `lhd3` + `ldat`)

`lhd3` (13 `u32`): word 0 `0x00d0_0bee`, word 2 the record count, word 3 `ceil(count / 4)`, word 4
the record size, word 7 the count rounded up to a multiple of 4, words 8 to 12 zero; `ldat` is
exactly `count * record size` bytes (cases 050, 055; every list of every fixture). In path lists
(record size 8) words 3 and 7 are allocation capacities in vertices and four times that (12 records:
4 and 16; 9 records: 4 and 16 in the shape paths of 105 and 240, 3 and 12 in the open mask path 233),
not a function of the count.

**Record header (8 bytes, every kind).**

| Field | Offset | Type | Meaning | Evidence | Conf. |
|---|---|---|---|---|---|
| time | 0 | i32 | in the property's time scale; **layer time** (roving moved key 2 from 25 600 to 26 509 = 1.0355078125 s) | 055 (0, 25 600, 64 000), 056, 058 | V |
| **interpolation in** | 4 | u8 | the incoming side: 1 = linear, 2 = bezier, 3 = hold | 202 (in linear, out hold: `01 03`), 203 (in hold, out linear: `03 01`), 204, 205, 052, 053 | V |
| **interpolation out** | 5 | u8 | the outgoing side, same codes | same | V |
| label | 6..7, bits 6..9 | u16 | keyframe label: label 9 turns the flags `0x0007` into `0x0247` | 057 | V |
| roving | 6..7, bit `0x0020` | u16 | `0x0007` becomes `0x0027` | 056 | V |

A hold key has no further field (the value is unchanged); a linear key has all ease slots 0. Keys
added through the scripting interface are linear in and out.

**The template follows the property header (`tdb4` bytes 3 and 7), not the record size.** With
`n` components:

| Template | `tdb4[7]` | Size | Used by | Evidence |
|---|---|---|---|---|
| plain | 0 | `8 + 40 n`: 48 (n=1), 88 (n=2), 128 (n=3) | Opacity, Rotation, Slider (1); Mask Feather, Rectangle Size (2); Scale (3) | 205, 206, 208, 221 |
| spatial | non-zero | `56 + 24 n`: 128 (n=3), 152 (n=4) | Position (3), colours (4) | 204, 210 |
| mask path | | 64 | the path property of an animated mask (the path itself is in `shap`) | 093 |
| marker | | 16 | `mrst` marker times | 122, 123, 249 |

A 128-byte record is a Scale key (plain, 3 components) or a Position key (spatial): a reader must
look at the header (206 against 204). Layout of the plain template: value (`n` x `f64`) at 8, then
`4 n` ease doubles. For `n = 1` they are **at 16 in speed, at 24 in influence, at 32 out speed, at
40 out influence**; for `n` of 2 and 3 only all-zero (linear) records were sampled, so the order of
the `4 n` doubles is not decoded (`Keyframe::plain_ease_slots` returns them as stored). Layout of
the spatial template: `u32` at 8 (1 normally, 0 after the spatial auto-bezier and continuous
switches were turned off together, 054), `f64` arc length of the segment to the next key at 16
(100 in 204 key 1, 0 on the last key and right after creation; safe to ignore), **ease at 24 in
speed, at 32 in influence, at 40 out speed, at 48 out influence**, value (`n` x `f64`) at 56, spatial
in tangent (`n` x `f64`, relative offset from the value) at `56 + 8 n`, out tangent at `56 + 16 n`.

| Field | Where | Meaning | Evidence | Conf. |
|---|---|---|---|---|
| ease, Position | spatial @24..55 | 204 key 1: in speed 5 / influence 0.2, out speed 50 / influence 0.8; key 2: in 7 / 0.6, out 9 / 0.4. Speed in pixels per second, influence a fraction. **After Effects reports key 1's in speed and the last key's out speed as 0, but the file keeps the requested values (5 and 9): ignore those two slots** | 204, 052 (preset: speed 0, influence 0.33) | V |
| ease, Opacity | plain n=1 @16..47 | 205 key 1: in 0.05 / 0.2, out 0.2 / 0.8; key 2: in 0.3 / 0.6, out 0.09 / 0.4. Speed is in stored units per second, i.e. 100 times smaller than the percent per second shown in the UI; influence a fraction | 205 | V |
| value, Scale | plain n=3 @8 | 206: `[1, 1, 1]` and `[2, 1.5, 1]` (fractions of the manifest percent) | 206 | V |
| value, Rotation, Slider | plain n=1 @8 | degrees / as set (keyed slider 10 and 90) | 221 | V (slider), E (rotation, 207 is not a fixture) |
| value, Mask Feather | plain n=2 @8 | pixels `[0, 0]` and `[40, 20]` | 208 | V |
| value, colour | spatial n=4 @56 | ARGB doubles 0..255: `[255, 255, 0, 0]` and `[255, 0, 127.5, 255]`; flags `0x0001`, tangents zero; the project-level `EfdG` copy of the effect has the same list | 210 | V |
| value, Position | spatial n=3 @56 | pixels | 055 (`[0,0,0]`, `[100,0,0]`, `[200,100,0]`), 056 | V |
| tangents | spatial @`56+8n`, `56+16n` | 054: key 1 out `[30, 60, 0]`, key 2 in `[-30, -60, 0]` | 054 | V |
| arc length | spatial @16 | 100 on key 1, 0 on key 2 | 204 | E |

API: `Keyframe::{interpolation_in, interpolation_out, kind, value, ease, plain_ease_slots, spatial}`;
`Property::keys()` hands the property's template to its records. Marker record (16 bytes): time,
`03 03` hold bytes, 8 stale bytes. Mask path record (64 bytes): the common fields only. Test:
`tests/values.rs`, `tests/keyframes.rs`.

### 3.6 Expressions

| Field | Chunk path | Meaning | Evidence | Conf. |
|---|---|---|---|---|
| source | `tdbs/Utf8`, after `cdat` / `list` and before `tdum` | raw UTF-8 text, no length prefix, no NUL, no engine tag | 060 (`wiggle(1,10)` on Opacity) | V |
| flags | `tdb4` 76, 77, 120 | all 1 when present, all 0 otherwise | 060, 040 | V |
| **disabled** | `tdb4` 119 | 1 when disabled (`expression.disable_all`); the only byte that differs between 060 and 213 | 060, 213 | V |
| static value | `cdat` | kept while an expression exists (Opacity 1.0) | 060 | V |

The expression engine is project-wide (`LIST ExEn/Utf8`, `javascript-1.0`). API:
`Property::expression_enabled`. Test: `tests/keyframes.rs`.

### 3.7 Effects (`tdgp[ADBE Effect Parade]/sspc`)

| Field | Chunk path | Offset | Type | Meaning | Evidence | Conf. |
|---|---|---|---|---|---|---|
| match name | the `tdmn` before the `sspc` | | text | `ADBE Slider Control`, `Pseudo/@@S1iPXtJFTHK4QaDbgJvLhw`, ... | 071, 073, 077, 080, 214 | V |
| instance name | `sspc/fnam/Utf8` | | text | `Slider Control` | 071, 073, 077, 080 | V |
| parameter count | `sspc/parT/parn` | 0 | u32 | including the implicit input layer and the built-in entry (3 for a slider) | 071 | V |
| parameter match name | `parT/tdmn` | | text | `<effect match name>-NNNN`; the last is `ADBE Effect Built In Params` | 071 | V |
| parameter type | `parT/pard` | 12 | u32 | 0 layer, 2 fixed slider (16.16), 3 angle, 4 checkbox, 5 colour, 6 point, 7 popup, 9 no data, 10 float slider, 12 path | 071, 073, 077, 080, 214, 215 | V |
| parameter name | `pard` | 16 | 32 ASCII | `Slider`, `Color`, `Menu`, `Blurriness`, `Shadow Color`, ... (empty for some checkboxes) | same | V |
| **default** | `pard` | 56 | by type | float slider: f64 (Slider 0, **Blurriness 25**); fixed slider and angle: 16.16 (drop shadow opacity 127.5, direction 135, distance 5, softness 0; Fill opacity 1.0 = 100 %); **colour: ARGB bytes (drop shadow `ff000000` black, Fill `ffff0000` red)**; popup: u32 index (1) | 071, 077, 080, 214, 215 | V |
| popup items | `parT/pdnm/Utf8` | | text | items separated by `\|` | 077 (`Item 1\|Item 2\|Item 3`), 214 | V |
| stored value | `sspc/tdgp/<match>-NNNN` | | `tdbs` | present only for a parameter that differs from its default; `tdsb` 1, `cdat` as in 3.4. **Absent record = the `pard` default** | see below | V |

Stored values by type (all `tdsb` 1):

| Type | Stored | Evidence | Conf. |
|---|---|---|---|
| float slider | `cdat[0]` as set (Slider 77, Blurriness 40); a keyed slider has 48-byte plain records and no `cdat` | 071, 216, 221 | V |
| colour | ARGB doubles 0..255 | 073; keyed: 210 | V |
| **checkbox** | `cdat[0] = 1.0` when on; off is the default and has no record (`tdb4[8..11]` = `00010004`). A checkbox parameter without a record reads as off | 217; the Repeat Edge Pixels checkbox of 214 | V |
| **point** | two doubles, a **fraction of the layer size**: `[111, 222]` is `[111/1920, 222/1080]` (`tdb4[3]` = 2, `[5]` = 15). The point default (`pard` @56, two 16.16 fractions 0.5) is not exposed | 218 | V |
| **angle** | plain degrees: 90 is `90.0` | 219 | V |
| **layer control** | the `-0001` record exists (`cdat` 0); the sibling `tdpi` (u32) is the **id of the chosen layer** (the new Null, `0x22`). In the implicit first parameter `tdpi` is the id of the layer that owns the effect | 220 | V |

The default of Gaussian Blur: the default-only file is byte-identical to the one with Blurriness 25
and has no value record; `pard` holds 25 (f64 @56 and f32 @120); setting 40 adds a record and leaves
`pard` at 25, so `pard@56` is the default and not the current value. The corpus manifest says the
default is 0, the files never contain 0; the decoder follows the files (**V** for the rule, **?**
for the 0-versus-25 conflict). API: `EffectParam::{stored_value, pard_default, value}`, `ParamValue`.
Test: `tests/effects.rs`.

### 3.8 Masks and bezier paths (`tdgp[ADBE Mask Parade]`)

| Field | Chunk path | Offset | Type | Meaning | Evidence | Conf. |
|---|---|---|---|---|---|---|
| inverted | `Mask Parade/mkif` | 0 | u8 | | 090 (0), 091 (1) | V |
| **mode** | `mkif` | 4 | u32 | Add 1, Subtract 2, **Intersect 3, Lighten 4, Darken 5, Difference 6**; the mode None was not sampled and is kept raw | 090, 091, 234-237 | V |
| name | `tdgp[ADBE Mask Atom]/tdsn/Utf8` | | text | `Mask 1` | 090 | V |
| feather, opacity, expansion | `ADBE Mask Feather`, `ADBE Mask Opacity`, `ADBE Mask Offset` | | see 3.4 | stored only when not at the default; feather can be keyed (plain n=2 records) | 090, 091, 208 | V |
| path property | `om-s[ADBE Mask Shape]/tdbs` | | | animated: `tdb4` byte 68 = 1 and a `list` of 64-byte records (time and common fields only) | 090, 093 | V |
| path shapes | `om-s/omks/LIST shap` | | | one per keyframe, one for a static path | 090, 093 | V |
| box | `shap/shph` | 4, 8, 12, 16 | 4 x f32 | left, top, right, bottom **normalised to the layer size** for masks (560 / 1920, 340 / 1080, 1360 / 1920, 740 / 1080). It is the box of the vertices **and** of the handles | 090, 232 | V |
| **open path** | `shph` | 3 | u8 | bit `0x08` set = open path (`0x09` against `0x01`); byte 20 is 1 in every sample, open or closed | 233 against 090, 232, 234 | V |
| points | `shap/list` (`lhd3` record size 8) | | f32 x, y | three per vertex in the order **vertex i, out handle i, in handle i + 1**, normalised inside the box (u = (x - left) / (right - left)); the last record is the in handle of vertex 0. A **tangent is the handle minus the vertex**, without a sign change | 090 (rect), 093 (two rects), 232 (ellipse: tangents `0.5523 x radius`, `[-220.91, 0]`, `[0, -110.46]`, ...), 233 (open path, 3 vertices) | V |

There is one `mkif` per parade in all samples (a single mask), so mode and inverted are reported for
the first mask only (`mkif` byte 8 is a running mask counter of the After Effects session and is
not decoded). Whether the box is normalised to the layer or the composition size is untested (both
1920 x 1080). Test: `tests/masks.rs`, with a synthetic check of the handle order.

### 3.9 Shape layers (`tdgp[ADBE Root Vectors Group]`)

A shape layer (type 4) holds `ADBE Vector Group` (user name `Group 1`) with `ADBE Vectors Group`
(the contents, in z-order, `tdsb` 0x401) and `ADBE Vector Transform Group`. Items are named by their
match name and carry their own user name. Colours are ARGB doubles 0..255. All sizes, positions and
path points are in pixels.

| Item (match name) | Property | Stored | Evidence | Conf. |
|---|---|---|---|---|
| `ADBE Vector Shape - Rect` ("Rectangle Path 1") | `ADBE Vector Rect Size` | f64 `[400, 300]` (2 components) | 238-242 | V |
| `ADBE Vector Shape - Group` ("Path 1") | `om-s[ADBE Vector Shape]` | the same `shph` + point list as a mask, with a box in **pixels** relative to the layer origin; vertices `[0,-100] [100,100] [-100,100]`, in tangents `[-10,-20] [30,20] [-60,10]`, out tangents `[40,0] [-20,30] [0,-50]`; the box covers vertices and handles: `-160, -120, 130, 130` | 240 | V |
| `ADBE Vector Graphic - Fill` | `ADBE Vector Fill Color` | `[255, 0, 255, 0]` | 238, 240, 242 | V |
| `ADBE Vector Graphic - Stroke` | `Stroke Color` `[255, 0, 255, 0]`, `Stroke Width` 8, **`Stroke Line Cap` 2.0 (round; butt = 1 is absent)**, **`Stroke Line Join` 2.0 (round; miter = 1 is absent)**, `Stroke Miter Limit` 4 with `tdsb` 3 once the join was touched | doubles | 239 | V |
| `ADBE Vector Filter - RC` ("Round Corners 1") | `ADBE Vector RoundCorner Radius` | f64 20 | 238 | V |
| `ADBE Vector Graphic - G-Fill` ("Gradient Fill 1") | `ADBE Vector Grad Start Pt` `[-200, 0]`, `ADBE Vector Grad End Pt` `[200, 0]` | 2 components, class 15; the gradient type (linear) and the colour stops stay at After Effects' default and are not stored | 241 | V (points) |
| `ADBE Vector Transform Group` | `ADBE Vector Position` | f64 `[50, 50]`; absent = `[0, 0]`, as are all other transform properties at default | 242 | V |

API: `Layer::shape_groups` (`ShapeGroup`, `ShapeItem`, `Stroke`). Other items (ellipse, polystar,
trim paths, repeater, dashes, ...) are listed as `ShapeItem::Other` with their match name; see
section 4. Test: `tests/shapes.rs`.

### 3.10 Text documents (`btds[ADBE Text Document]/LIST btdk`)

`btdk` is not a chunk sequence but a PostScript-like dictionary (`/key value` pairs, `<< >>`, `[ ]`,
numbers, `true` / `false`, `/names`, strings as `(` + `fe ff` + UTF-16BE + `)` with `\` escapes). The
verified paths from the top level (keys `/98 /0 /1`; `RUN` = `/1/1[0]/0`):

| What | Path | Evidence | Conf. |
|---|---|---|---|
| text | `RUN/0`; a trailing `\r` ends the paragraph. **A hard line break is `\r` too: `A\rB` is stored `A\rB\r`** | 110 (`A`), 113 (`Hello`), 243 | V |
| font size | `RUN/6/0[n]/0/0/6/1` (100 by default) | 110, 113 (60) | V |
| fill colour | `RUN/6/0[n]/0/0/6/53/0/1` = `[A R G B]` floats 0..1 | 110 (white), 113 (`[1 1 0.5 0]`), 248 | V |
| **paragraph runs** | `RUN/5/0[n]/1` = characters including the `\r` (`A\rB\r`: `[2, 2]`; `Hello\r`: `[6]`) | 243, 245 | V |
| **style runs** | `RUN/6/0[n]/1` = characters per style run (`A\rB\r`: `[4]`). **A range style splits `Hello\r`: characters 1 to 3 red gives `[1, 2, 3]` with the fills white, `[1 1 0 0]`, white** | 243, 248 | V |
| **justification** | `RUN/5/0[n]/0/0/5/0`: 0 = left, 2 = centre; right is presumably 1, the justified variants are unknown (kept raw) | 245 (0), 246 (2) | V (0, 2) |
| **box (paragraph) text** | frame dictionary `/0/8/0[0]/0`: `/1/0` = 32 floats = 16 x, y pairs tracing the rectangle (corners `[-200,-100] [200,-100] [200,100] [-200,100]`), `/2/0` = 1; both keys are absent for point text. Alignment changes only the glyph geometry, not the frame | 245, 246 (box 400 x 200, top-left `[-200, -100]`), 247 (point text: absent) | V |
| **tracking** | `RUN/6/0[n]/0/0/6/8` = 0, 100 in 247 | 247 | V |
| font | `.../6/0` = index into the font list `/0/1/0[i]/0/0/0` (`TimesNewRomanPSMT`, ...) | 110 | E |

API: `TextDocument` (`paragraphs`, `style_runs`, `text_box`, `paragraph_texts`). Test:
`tests/text_markers_footage.rs`.

### 3.11 Markers

`mrst[ADBE Marker]` holds a `tdbs` with a `list` of 16-byte records (the times; `tdb4` byte 12 is the
time scale) and one `LIST mrky/LIST Nmrd` per marker. Layer markers live in the layer's own tree,
composition markers in the `SecL` list named `Markers`.

| What | Chunk path | Meaning | Evidence | Conf. |
|---|---|---|---|---|
| time | the key list | 25 600 = 1 s | 122, 123, 249 | V |
| **duration** | `Nmrd/NmHd` (20 bytes: u32 4, u32 0, u32 duration, u32 600, u32 0) | duration / 600 = the marker duration in seconds (1 s is 600; 0 for none); 600 is the units per second | 249 (1 s), 122 (0) | V |
| **strings** | five `Utf8` children of `Nmrd`, in the order **comment, chapter, URL, frame target, cue point name**; unused ones are empty | `LM`, `Chap`, `http://a.example/u`, `_blank`, `CueX` | 249 | V |

Test: `tests/text_markers_footage.rs`.

### 3.12 Cameras and lights

Both have no source item (`ldta@40` is 0 for a camera, `0xffff_ffff` for a light), type 2 or 1, the 3D
bit and byte 38 bit `0x40` (3.1); `ldta@136` is the light type. Their properties sit in the same
groups as on other layers, with these conventions:

| What | Path | Stored | Evidence | Conf. |
|---|---|---|---|---|
| camera Position, default (auto state) | `ADBE Transform Group/ADBE Position`, `tdb4` 76 and 77 = 1 although no expression exists | `[0, 0, -2666.67]` (x and y are 0 whatever the composition size; z = -zoom). A **two-node** camera reports `[960, 540, -2666.67]` (x, y = composition centre), a **one-node** camera reports `[0, 0, -2666.67]` | 256, 251 | V |
| camera Position, set by the user | same, `tdb4` 76 and 77 = 0 | the absolute value `[100, 200, -1000]` | 253 | V |
| camera point of interest | `ADBE Anchor Point` | `[960, 540, 0]` in **pixels** (not a fraction); the record's `tdsb` is 1 on a two-node and 3 on the one-node camera (not asserted) | 251, 253, 256 | V (value), E (`tdsb`) |
| light Position, default | same as the camera | `[0, 0, -1333.33]` | 254 | E |
| light intensity | `ADBE Light Options Group/ADBE Light Intensity` | f64 50.0 percent, `tdsb` 1; absent at the default 100 | 254 (50), 252 (absent) | V |
| light type | `ldta@136` | 1 spot, 2 point | 254, 252 | V |

A point light made by converting a spot light has a different property set (`Orientation`, `Rotate
X/Y/Z`, `Cone Angle`, `Cone Feather 2` in the default state) that a spot light lacks (252); whether that
is an artefact of the conversion is untested. API: `Layer::camera_position`,
`CameraPosition::reported`, `Layer::camera_point_of_interest`, `Layer::light_intensity`. Test:
`tests/layer_switches.rs`.

### 3.13 What the tests cover

`tests/header.rs`, `values.rs`, `keyframes.rs`, `layer_switches.rs`, `effects.rs`, `masks.rs`,
`shapes.rs` and `text_markers_footage.rs` compare the decoded fields with the manifest values of the
cases listed above; `typed.rs` checks the structural relations on every property and every list of
every fixture; `robustness.rs` runs the whole typed view over damaged copies of the decoded fixtures
(byte flips, truncation, damaged chunk bodies, every component count and template flag of the
keyframe header, damaged marker and layer-reference chunks) and over extreme values.

## 4. Not decoded

Fields that the corpus leaves open (**?**), only suggests (**H**), or shows without a manifest value to
assert against (**E**, not exposed), and areas that were not sampled. None of these is decoded by the
crate; the raw bytes stay reachable through the chunk tree.

The fixtures are the small files of the corpus. Cases that exist in the corpus but have no
fixture (the budget for all fixtures is 8 MB): 200, 201, 299 (save-noise pairs), 207 (Rotation keys),
209 (Rectangle Size keys), 244 (point text reference), 250 (comp marker at 2 s), 255 (default spot
light, identical to 121), 257 and 258 (precomps of a 640 x 360 solid and of a 640 x 360 composition),
and most of batch 1 (011, 020, 036, 045, 050, 051, 061, 070, 072, 074-076, 078, 079, 092, 100-105,
111, 112, 114, 120, 121, 125). What they establish is listed in the tables above with their case
numbers; the tests do not assert it.

**Layer header (`ldta`)**

- Quality Wireframe, track mattes, frame blending, time remapping (`ADBE Time Remapping` records) and `autoOrient` on non-camera layers (not sampled).
- Time stretch other than a whole multiplier (50 %, 33.3 %), layer names longer than 31 characters, and the start-time denominator of layers made by the API against the precomp operation (258 against 257: 25 600 against 1, **V** in the corpus, no fixture).
- Bytes 56 (1 on AV, shape and text layers, 0 on camera and light; meaning ?), 96 and 108 (2 and 1 on AV layers), and the camera extra at 148 / 152 (a double that equals the 36 mm film size in points; **E** for the value, **H** for the meaning).

**Composition header (`cdta`, `PRin`)**

- Bytes 0, 16 (600: **H**, the 1/600 s legacy time base), 24, 32, 40 (cache-like: they hold values the session used earlier, do not use them), 48, 168, 176, 184, 196 and 200 (**?**).
- Non-integer pixel aspect ratios (0.9091), the shutter phase, the start time / start frame, preserve-frame-rate and preserve-resolution switches, draft 3D, `Hide shy layers`, frame blending, the 8-versus-16-bit project depth (not sampled).
- `PRin/prda` (12 bytes for Classic 3D, 52 for Advanced 3D: renderer parameters, **E**).
- Item `idta` byte 20, `idpc` (**V** in the corpus: 1 when the composition contains a nested composition), `head`, `nhed`, `nnhd`, `Ewst`, the `f*` records, `Gide`, `PTRE`, `LRdr` and `LSIf` bodies. `head` byte 19 is the save counter and is the only byte of the container that changes in a Save As without an edit; item `idta` bytes 80 to 83 are the modification time (seconds since 1904-01-01); `nhed`/`nnhd` hold an id of the After Effects session; `mkif` byte 8 is a running mask counter; `pgui` chunks are random GUIDs. All of that is edit-history or session state (**V** in the corpus) and is not decoded.

**Properties and keyframes**

- `tdb4` bytes 4, 8..11 (a descriptor per property class: **E**), 16 (0.0001: **E**), 24 (the composition pixel aspect on spatial properties: **E**), 32..48 (1.0: **E**), 56..67 (a class pair: **E**), 72..78 except 76 and 77, 79 (**E**); `tdsb` bits other than 0x1 and 0x2 (group flags `0x401`, `0x11`, ...); `tdum` / `tduM` (**?**).
- The order of the ease doubles of plain records with 2 or 3 components (the samples are linear; component-major is **H**), and the ease of colour keys (zero in the linear sample).
- The 8 stale bytes of the marker record and the bytes after the common fields of the mask path record (uninitialised memory).
- Expressions in effect parameters, multi-line and non-ASCII expressions, the ExtendScript engine.
- The sparse-storage rule for a value set back to its default (is the record removed again?) and an explicit default (a Blurriness set to 0 or 25): **?**. Whether a writer may rely on defaults is therefore open.

**Effects**

- `pard` byte 48 (flags), 60 (popup choice count as a `u16`: **E**), 68 / 72 (point default: **E**), 104 / 108 (float slider valid minimum and maximum: **E**), 116 / 120 (slider range and default: **E**), 124, 128..147 (fixed slider ranges: **E**), and the stale memory after the parameter name. Minimum and maximum are not exposed; the point default is not exposed.
- How "None" differs from "the owning layer" in a Layer Control (076 stores the owning layer's id, **?**), the stored units of fixed sliders and popups, effects that are switched off, several effects on one layer, renamed effects (not sampled).
- The Gaussian Blur default conflict (3.7).

**Masks, shapes**

- Whether `mkif` is stored once per mask (one `mkif` per parade in every sample), the mode None, locked, colour, feather falloff, rotobezier, keyed expansion, masks on a layer whose size differs from the composition (whether the box is normalised to the layer or the composition size).
- Shape layers: ellipse and polystar items (type, points, rotation, roundness), rectangle roundness and position, open shape paths, trim paths, repeater, dashes, taper, wave, fill rule, merge paths, group scale / rotation / anchor / opacity / skew, several groups, gradient type, stops and highlight, the default of stroke colour, width, fill colour and rectangle size. The batch-1 cases (100 to 105: rectangle, ellipse, polystar, path, trim, repeater) decide some of them, but have no fixture.

**Text**

- Right and justified alignments (1 and 3 to 6), bold / italic, leading, stroke, other fonts, per-character offsets, non-BMP characters, text animators and range selectors, source-text keyframes, and the roughly 700 other leaves of the dictionary.

**Camera, light, footage, markers**

- Camera zoom, focal length, depth of field, film size; light parallel / ambient, colour, cone, falloff, casts shadows.
- Marker label colour, navigation versus event cue points, several markers, fractional times, non-ASCII comments, markers whose duration is not a whole second (the 600 units per second field).
- Footage with audio, image sequences, footage interpretation, missing files, other alias JSON keys, time remapping, precomps with another size / frame rate / start, solids with alpha.

## 5. Method

The format is established only from files we generate and compare, never from another program's
parser or from Adobe's code.

1. **Generate.** A base project is built once. For each case exactly one thing is changed through
   After Effects' own scripting interface (a layer switch, a composition property, a static value,
   a keyframe, an effect, a mask, a text edit), the project is saved as `NNN-name.aep`, the change
   is undone and the base state is re-checked before the next case. A manifest lists every case
   with the exact change and the values used; the same state is also saved as the XML flavour of
   the project, which names the chunks and can be used as a cross-check. Batch 2 repeated this in
   later sessions, each case again from the same loaded base, and added two saves without an edit
   to isolate what a save changes.
2. **Diff.** A walker (our own code) parses each file into the chunk tree, and each case is
   compared with the base and with its parent case: chunk path, size and the offsets and bytes
   that changed. Save noise (a save counter, time stamps, the growing XMP history, per-session
   ids) is filtered. Because one change produces a handful of chunk differences, the field that
   holds a value is found by the byte offsets, and checked by changing the value (`040` against
   `041`, `055` against `051`, `202` against `203`).
3. **State a hypothesis, then verify it.** A statement enters this document only when the manifest
   value and the decoded value agree in the case files (the table above names them) and the same
   relation holds on every other file where it applies. A decoder script outside this repository
   checks every layout against the manifest values; the tests in `crates/aep/tests/` are the
   verification inside it: they decode the fixtures and compare with the manifest values (layer
   switches, in / out / start times, composition size, frame rate, work area and background,
   Position `100, 200, 0`, Scale 50 %, key times and ease, the interpolation sides, anchor
   fractions of two solid sizes, Slider 77, Blurriness 40, point, angle and checkbox values, mask
   vertices and tangents, mask modes, shape sizes and colours, text and fill colour, text runs,
   marker times and strings, camera positions, light type and intensity, solid colours), and check
   the structural relations on every chunk of every fixture. A field with several cases that agree
   is marked V in section 3; a field seen only in the case diffs is marked E.
4. **Skip what is not verified.** Anything without an agreeing manifest value, or with a
   conflicting case, stays undecoded (section 4).

The corpus itself and the helper scripts live outside this repository. Larger production projects
were examined locally for statistics only (which chunk ids occur and how often); no layout in this
document comes from them and none of them, nor any content of them, is in the repository. Only the
103 fixtures above are committed, as the contributor's own test data (see `ATTRIBUTION.md`); the
repository holds no Adobe sample project, preset or asset.

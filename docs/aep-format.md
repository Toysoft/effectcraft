# After Effects project format (`.aep`)

> **Last reviewed:** 2026-10-13 · **Last updated:** 2026-10-13 · **Change:** verified field layouts for layers, compositions, static values, keyframes, expressions, effects, masks, text, markers, footage and precomps added to the typed view of `effectcraft-aep` · **Target:** Adobe After Effects 2026

What `crates/aep` ([`effectcraft-aep`](../crates/aep/README.md)) reads and writes, and the evidence
for each statement. Everything here was observed on project files we generated ourselves; nothing
comes from a third-party parser, specification or source. This is the container and a set of
verified field layouts, not a project importer: turning a parsed `.aep` into an EffectCraft
document is future work (see [gaps.md](gaps.md) G4 and [file-format-parity.md](file-format-parity.md)).

Evidence is cited as **case numbers**: the files `NNN-*.aep` of our synthetic corpus, each made by
one scripted change to a base project (cases generated later are not used here). The base project `000-base` is one composition `C` (1920x1080, 25 fps, 5 s) holding one
red solid layer `L` in the folder `Solids`. 52 of these files are the test fixtures in
`crates/aep/tests/fixtures/` (3.9 MB together); the tests named below run on them. The container of
every fixture is byte-identical to the generated file, with two exceptions that keep the fixtures
small and free of local data: the XMP packet after the container (which grows by 1.7 KB with every
save of the corpus chain) is replaced by the 5.8 KB packet of the base project in all fixtures but
`001`, and in `126` the file path and machine name inside the footage alias JSON are replaced by
neutral text of the same length.

Confidence codes in the field tables: **V** = a test in `crates/aep/tests/` asserts that the decoded
value equals the value the case was made with (the corpus manifest); **E** = decoded and consistent
in the case diffs, but no manifest value to assert against (the field is still exposed only where
a test pins what the fixtures contain); fields that the corpus does not decide (**?**) or only
suggests (**H**) are not decoded; they are listed in section 4.

## 1. Container

| Fact | Evidence |
|---|---|
| The file starts with `RIFX`, a big-endian `u32` size and the form type `Egg!`; all integers in the structure are big-endian. | All 79 files. |
| The declared size is `4 + the chunks`; it ends before the end of the file. The bytes after it are an XMP packet (`<?xpacket begin` ... `<?xpacket end=...?>`, UTF-8 XML, 5.8 KB in `000`) that grows with every save. | All 79 files; test `header_and_trailer_facts`. |
| A chunk is a 4-byte id, a big-endian `u32` body size and the body; an odd-sized body is followed by one pad byte, always `0` in the corpus (8639 odd bodies). | Test `odd_sized_chunks_keep_their_pad_byte_even_when_not_zero` keeps any value. |
| The chunks of every file tile the container exactly: no leftover bytes, no chunk larger than its parent. | 79 of 79 files parse without a repair. |
| A `LIST` chunk's body is a 4-byte list type followed by chunks. List types seen include `Fold`, `Item`, `Layr`, `tdgp`, `tdbs`, `list`, `Pin ` and `FEE ` (the space is part of the type). | `000`; test `list_types_ending_in_a_space_are_kept`. |
| Lists can be empty (size 4, only the type): `Ewst`, `Pefl`, `CPPl`, `LItm`. | `000`; test `empty_list_and_list_shorter_than_its_type`. |
| Nesting: at most 13 levels in the corpus. | All 79 files; test `nesting_depth_is_below_the_limit`. |
| `LIST btdk` is not a chunk sequence: its body (7775 bytes in `110`) is text. A reader must keep it opaque. Only files with a text layer have it. | `110`, `111`, `112`, `113`, `114`; test `raw_text_lists_are_kept_opaque`. |
| Three non-`LIST` ids hold a chunk instead of data: `tdsn`, `pdnm`, `fnam`. Each holds exactly one `Utf8` chunk (a name or label, UTF-8 text, possibly empty). | `tdsn` 17 186 times, `fnam` 22, `pdnm` 14 over the corpus; test `nested_chunk_ids_hold_one_utf8_chunk`. |
| Ids can end in a space; the space is part of the id. In the corpus this happens for the list types `Pin ` and `FEE `. All ids are 4 printable ASCII bytes. | `Pin `, `FEE ` in 78 of 79 files (all but `001`); tests `list_types_ending_in_a_space_are_kept`, `ids_ending_in_a_space_are_kept`. |
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
headers, values with their units, keyframes, effects, masks, text, markers, footage) are in section 3.
Everything else is reachable as raw chunks.

| Fact | Evidence |
|---|---|
| Project items are `LIST Item` children of the top-level `LIST Fold`; a folder holds its children in a nested `LIST Sfdr`. | `000`: composition `C` and folder `Solids` at the top, the solid's footage item inside `Solids`. |
| The item kind is the first big-endian `u16` of `idta`: 1 folder, 4 composition, 7 footage. | `000` (`C`, `Solids`, the solid `L`); `001` has no composition. |
| The item name is the item's `Utf8` child (empty for a solid's footage item). | `000` (`C`, `Solids`), `036` (comp renamed: the `Utf8` grows from 1 to 12 bytes, `Renamed Comp`). |
| A composition item has a `cdta` whose `u32` at byte 4 is the frame duration (1024) and the `u32` at byte 8 the timescale: 25 600 units per second at 25 fps, so `fps = timescale / frame_duration`. Keyframe times are in these units. | `000` (1024, 25 600), `031` (frame rate 30: 1024, 30 720); `055` (key at 2.5 s = 64 000 units). |
| The layers of a composition are its `LIST Layr` children, top layer first. (The `DLay`, `SLay`, `CLay` and `SecL` lists next to them are not layers: their `ldta` names are `Default`, `Front`, `Left`, `Top`, `Back`, `Right`, `Bottom`, `Custom View 1` to `3` and `Markers`.) | `000` (one layer), `110` (text layer on top of `L`). |
| The layer name is the `Utf8` child of the layer; it is empty while the layer shows its source's name. | `010` (`L` renamed: 0 to 7 bytes, `Renamed`), `110` (`T`). |
| The 3D layer switch is bit `0x04` of byte 38 of `ldta`. | `015` (only that byte changes from `00` to `04`; the 3D properties appear), `000`. |
| A layer holds its property tree in a `LIST tdgp`. Inside a group, a `tdmn` chunk (the match name: ASCII, NUL padded to 40 bytes) precedes the list it names: `tdgp` (a group), `tdbs` (a property) or another list (`otst`, the effect instance `sspc`). A `tdmn` named `ADBE Group End` closes the group and names nothing. | `000` (`ADBE Transform Group`), `071` (`ADBE Effect Parade`, `ADBE Slider Control`). |
| Transform properties are stored sparsely: a property at its default has no `tdbs` at all. | `040`, `041`, `043`, `044`: each adds exactly one `tdmn` and one `tdbs` (`ADBE Position`, `ADBE Scale`, `ADBE Opacity`, `ADBE Anchor Point`) to `000`; test `static_value_is_the_first_dims_doubles_of_cdat` (`000` has no Position). |
| A property (`tdbs`) holds `tdsb`, `tdsn`, the 124-byte header `tdb4`, then the static value `cdat` or a keyframe `LIST list`. `tdb4` begins `db 99 00`; byte 3 is the number of components (Position 3, Scale 3, Opacity 1, Slider 1); byte 68 is 1 exactly when the property has a `list`. | `040`, `041`, `043`; `050` (the `cdat` is replaced by a `list`, byte 68 goes 0 to 1); `055`; test `property_header_facts_hold_in_every_property_of_every_fixture`. |
| The static value is the first `dims` big-endian doubles of `cdat`. Percent properties are stored as fractions: Scale 50 % is `0.5, 0.5, 1.0`, Opacity 25 % is `0.25`. | `040` (Position `100, 200, 0`), `041`, `043`, `071` (Slider `77`). |
| A `LIST list` is `lhd3` (52 bytes, 13 `u32`) and `ldat`. Word 0 is `0x00d00bee`, word 2 the record count, word 4 the record size, words 8 to 12 are zero; `len(ldat) = count * record size`; a list without records has no `ldat`. In the keyframe lists of properties, word 3 is `ceil(count / 4)` and word 7 the count rounded up to a multiple of 4 (not in the empty `Gide` lists). | The first four relations hold for every `list` of every fixture (test `list_header_facts_hold_in_every_list_of_every_fixture`); word 3 and 7 on the keyframe lists of `050` (1 record) and `055` (3 records of 128 bytes, capacity 4). |
| A keyframe record starts with its time as a big-endian `i32` in the composition's time units. | `055` (0, 25 600, 64 000 for keys at 0 s, 1 s, 2.5 s), `058` (0 and 51 200 for keys at 0 s and 2 s). |
| A 128-byte record is a spatial (Position) key; its x, y, z are doubles at bytes 56, 64 and 72 (the `u16` at byte 6 is 7 in a plain key; a label or the roving flag changes it, see 3.5). | `055` (`0,0,0`, `100,0,0`, `200,100,0`), `056`, `057`. |

## 3. Verified field layouts

All integers are big-endian unless stated; offsets are byte offsets inside the chunk body; `f64`
is an IEEE-754 double, `f32` a float. Chunk paths start at the project item; `Layr` is a real layer
(`LIST Layr`), `tdgp[X]` is the `LIST tdgp` named `X` by the `tdmn` that precedes it. Each row lists
the cases that decide it. A row marked V is asserted by a test that compares the decoded value with
the value from the corpus manifest (test file in parentheses at the end of each table).

### 3.1 Layer header (`Layr/ldta`, 164 bytes; the decoder needs the first 136)

| Field | Offset | Type | Meaning | Evidence | Conf. |
|---|---|---|---|---|---|
| id | 0 | u32 | layer id, unique in the project; parent links use it | 000 (`0x1b`), 022 | V |
| quality | 5 | u8 | 2 = Best, 1 = Draft (other values are kept as `Other`) | 000, 023 | V |
| stretch | 8 | u32 | time stretch as a whole multiplier: 1 = 100 %, 2 = 200 % | 000, 018 | V (1, 2) |
| start | 12 / 16 | i32 / u32 | layer start time = num / den seconds; den = composition timescale (25 600), 1 in a precomp layer | 000, 021 (12 800 / 25 600 = 0.5 s), 124 | V |
| in point | 20 / 24 | i32 / u32 | **relative to the start**: absolute = start + in * stretch | 000, 019 (25 600 / 25 600 = 1 s), 021 (stays 0 while the layer starts at 0.5 s) | V |
| out point | 28 / 32 | i32 / u32 | same convention; 5 s = 128 000 / 25 600 | 000, 018 (stays 5 s, the layer ends at 10 s), 019 (6 s), 021 (5.5 s) | V |
| name is user-set | 37 bit `0x01` | flag | clear while the layer shows its source's name (000, 124), set when named | 000, 010, 022, 110, 113, 124 | V |
| guide layer | 37 bit `0x02` | flag | | 026 | V |
| adjustment layer | 38 bit `0x02` | flag | | 025 | V |
| 3D layer | 38 bit `0x04` | flag | | 015 | V |
| solo | 38 bit `0x08` | flag | | 014 | V |
| null object | 38 bit `0x80` | flag | set on a layer created as a null | 022 | V |
| motion blur | 39 bit `0x08` | flag | | 017 | V |
| locked | 39 bit `0x20` | flag | | 013 | V |
| shy | 39 bit `0x40` | flag | | 012 | V |
| collapse transformations | 39 bit `0x80` | flag | continuously rasterize; set from creation on shape and text layers | 024 | V |
| source id | 40 | u32 | `idta@16` of the source item (solid, footage or comp); 0 for shape, text and camera layers, `0xffff_ffff` for lights (`None` in the API) | 000 (`0x1a`), 022, 124, 126 | V |
| label | 61 | u8 | label colour index (1 red, 5, 8 blue, 15, ...) | 000 (1), 124 (15), 126 (5) | V |
| name copy | 64..95 | ASCII, NUL padded | copy of the user-set name; empty while the layer is unnamed | 000 (empty), 010 (`Renamed`) | V |
| blend mode | 99 | u8 | 2 = Normal, 5 = Multiply | 000, 016 | V |
| layer type | 128 | u32 | 0 AV (solid, footage, precomp, null), 1 light, 2 camera, 3 text, 4 shape | AV: 000, 022, 126; text: 110, 113; shape, camera and light layers (100, 120, 121) are not among the fixtures | V (0, 3) |
| parent id | 132 | u32 | 0 = no parent | 000, 022 (L gets the id of the new null layer) | V |

Test: `tests/header.rs`. The layer name itself is the `Utf8` child of the `Layr` (empty while the
name is the source's); the layer then shows the name of its source item (`Project::layer_display_name`).

### 3.2 Composition header (`Item/cdta`, 204 bytes; the decoder needs the first 176) and renderer

| Field | Offset | Type | Meaning | Evidence | Conf. |
|---|---|---|---|---|---|
| frame duration | 4 | u32 | time units per frame (1024) | 000, 031 | V |
| timescale | 8 | u32 | time units per second (25 600 at 25 fps, 30 720 at 30 fps); fps = timescale / frame duration | 000, 031 | V |
| work area start | 28 | u32 | in time units | 035 (25 600 = 1 s) | V |
| work area end | 36 | u32 | start + duration in time units; `0xffff_ffff` = the whole composition | 000, 035 (76 800 = 3 s) | V |
| duration | 44 | u32 | in time units | 000 (128 000 = 5 s), 031 (153 600 at 30 fps), 032 (256 000) | V |
| background | 52..54 | u8 R, G, B | 0..255 per channel (8 or 16 bit is unknown: only 0 and 255 were sampled) | 000, 033 | V (0 / 255) |
| motion blur | 139 bit `0x08` | flag | the composition's switch | 038 | V |
| width, height | 140, 142 | u16, u16 | | 000, 030 (1280 x 720) | V |
| pixel aspect | 144 / 148 | u32 / u32 | numerator / denominator | 000 (1 / 1), 037 (2 / 1) | V |
| frame rate | 156 | u32 | 16.16 fixed point (25 = `0x0019_0000`) | 000, 031 | V |
| shutter angle | 172 | u32 | degrees | 000 (180), 034 (90) | V |
| renderer | `LIST PRin` / `prin` 4 | 32 ASCII | match name: `ADBE Escher` in the base, `ADBE Calder` in 039 | 000, 039 | V |
| renderer name | `prin` 52 | 32 ASCII | display name: `Classic 3D`, `Advanced 3D` | 000, 039 | V |
| composition name | `Item/Utf8` | text | | 000 (`C`), 124 (`Pre`) | V |

The corpus manifest calls the classic renderer `ADBE Advanced 3d`; the file stores
`ADBE Escher` / `Classic 3D`. Test: `tests/header.rs`.

### 3.3 Items, footage and precomps

| Field | Chunk path | Offset | Type | Meaning | Evidence | Conf. |
|---|---|---|---|---|---|---|
| item kind | `Item/idta` | 0 | u16 | 1 folder, 4 composition, 7 footage | 000 | V |
| item id | `Item/idta` | 16 | u32 | what a layer's source id refers to (`iide` holds the same number little-endian) | 000 (`0x1a` solid, `0x0d` comp), 022, 124 | V |
| item label | `Item/idta` | 58 | u8 | label colour of the item | 000 (1 solid), 124 (15 for `Pre`) | V |
| used comp | `Item/idpi` | 0 | u32 **little-endian** | a composition that uses another as a precomp layer gets one `idpi` per used composition | 124 (`C` uses `Pre`) | V |
| footage type tag | `Pin /sspc` | 22 | 4 ASCII | `Soli` for a solid, `png!` for a png | 000, 126 | V |
| footage size | `Pin /sspc` | 30, 34 | u32, u32 | width, height in pixels | 000 (1920 x 1080), 022 (100 x 100 null), 126 (8 x 8) | V |
| footage name | `Pin /opti` | 26 (solid), 58 (file) | ASCII, NUL terminated | the item's own `Utf8` name is empty for footage | 000 (`L`), 022 (`Null 1`), 126 (`test8x8.png`) | V |
| solid colour | `Pin /opti` | 10, 14, 18, 22 | 4 x f32 | ARGB, 0..1: red [1, 0, 0] is `1, 1, 0, 0` | 000 | V (red only) |
| file path | `Pin /Als2/alas` | key `fullpath` | JSON string | alias JSON of a file footage item | 126 (the fixture holds a neutral path) | V |
| precomp linkage | `Layr/ldta` | 40 | u32 | the layer's source id is the id of the precomp's item | 124 | V |

Test: `tests/header.rs` (precomp), `tests/text_markers_footage.rs` (footage).

### 3.4 Properties: sparse storage, static values and units

A layer's property tree contains a `tdbs` only for a property whose value differs from the default
that After Effects computes for the layer and composition. A reader supplies the defaults:

| Property (match name) | Stored | Unit in the API | Default when absent | Evidence | Conf. |
|---|---|---|---|---|---|
| `ADBE Position` | f64 x, y, z | pixels | the composition centre `[width / 2, height / 2, 0]` | 000, 040 (`100, 200, 0`), 030 (the comp becomes 1280 x 720, L stays 1920 x 1080: `960, 540, 0` is now stored because it differs from the new centre) | V |
| `ADBE Anchor Point` | f64 x, y as a **fraction of the layer size**, z in pixels | px = fraction * layer size | `0.5, 0.5, 0` (the layer centre) | 000, 044 (`10, 20` px is `10/1920, 20/1080`) | V (layer size taken from the source; whether it is the layer's or the composition's size is not decided, both were 1920 x 1080) |
| `ADBE Scale` | f64 fractions | percent (fraction * 100) | 100 % | 000, 041 (`0.5, 0.5, 1.0`) | V |
| `ADBE Rotate Z` | f64 degrees | degrees | 0 | 000, 042 | V |
| `ADBE Opacity` | f64 fraction | percent (fraction * 100) | 100 % | 000, 043 (`0.25`) | V |
| mask feather | f64 x, y | pixels | `[0, 0]` | 090, 091 (`20, 20`) | V |
| mask opacity | f64 fraction | percent | 100 % | 090, 091 (`0.5`) | V |
| mask expansion `ADBE Mask Offset` | f64 | pixels | 0 | 090, 091 (5) | V |
| effect slider | f64 | as set | the `pard` default | 071 (77) | V |
| effect colour | 4 x f64, ARGB 0..255 | ARGB fractions | the `pard` default (not decoded) | 073 (`[0.2, 0.6, 1]` is `255, 51, 153, 255`) | V |

Property record (`LIST tdbs`: `tdsb`, `tdsn`, `tdb4`, then `cdat` or a `LIST list`, optionally a
`Utf8` expression, `tdum`, `tduM`):

| Field | Chunk path | Offset | Type | Meaning | Evidence | Conf. |
|---|---|---|---|---|---|---|
| magic | `tdb4` | 0..2 | `db 99 00` | every property | all fixtures | V |
| components | `tdb4` | 3 | u8 | values in `cdat` / in each key (1, 2, 3, 4 = ARGB) | 040, 041, 043, 073, 091 | V |
| static flag | `tdb4` | 5 bit 0 | flag | set while the property has no keys (Position: 15 static; the bit is clear on the animated Position and Opacity) | 040, 055, 058 | V |
| time scale | `tdb4` | 12 | u32 | units per second of the keyframe times (25 600; 30 720 at 30 fps) | 055, 058, 122 | V |
| has keys | `tdb4` | 68 | u8 | 1 exactly when the property has a `LIST list` | every fixture (test) | V |
| expression flags | `tdb4` | 76, 77, 120 | u8 | all three are 1 when an expression exists; which one is the enable switch is unknown | 060 | V (set) |
| state | `tdsb` | 0 | u32 | 1 = written (non-default value, keys or expression), 3 = default state | 040, 071, 073 (1); 3 on the unwritten properties of 000 | V (1), E (3) |
| static value | `cdat` | 0 | `components` x f64 | the first `components` doubles are the value | 040, 041, 043, 071 | V |
| user name | `tdsn/Utf8` | | text | `-_0_/-` means "no custom name" | 091 (`Mask 1`) | V |

Test: `tests/values.rs`, `tests/typed.rs`.

### 3.5 Keyframes (`tdbs/list`: `lhd3` + `ldat`)

`lhd3` (13 `u32`): word 0 `0x00d0_0bee`, word 2 the record count, word 3 `ceil(count / 4)`, word 4 the
record size, word 7 the count rounded up to a multiple of 4, words 8 to 12 zero; `ldat` is exactly
`count * record size` bytes (cases 050, 055; every list of every fixture). Kinds by record size:

| Record | Size | Used by | Evidence |
|---|---|---|---|
| spatial | 128 | Position (3 components) | 054, 055, 056, 057 |
| scalar | 48 | Opacity | 058 |
| mask path | 64 | the path property of an animated mask (the path itself is in `shap`) | 093 |
| marker | 16 | `mrst` marker times | 122, 123 |

Fields common to all records:

| Field | Offset | Type | Meaning | Evidence | Conf. |
|---|---|---|---|---|---|
| time | 0 | i32 | in the property's time scale; **layer time** (roving moved key 2 from 25 600 to 26 509 = 1.0355078125 s) | 055 (0, 25 600, 64 000), 056, 058 | V |
| interpolation | 4 and 5 | u8, u8 | 1 = linear, 2 = bezier, 3 = hold; the two bytes are equal in every sample, so which one is the incoming and which the outgoing side is unresolved. The names are likely rather than proven | 055 (1, 1), 052 (2, 2), 053 (3, 3 on key 1, 1, 1 on key 2) | V (values), unresolved (side) |
| label | 6..7, bits 6..9 | u16 | keyframe label: label 9 turns the flags `0x0007` into `0x0247` | 057 | V |
| roving | 6..7, bit `0x0020` | u16 | `0x0007` becomes `0x0027` | 056 | V |

Spatial record (128 bytes, Position):

| Field | Offset | Type | Meaning | Evidence | Conf. |
|---|---|---|---|---|---|
| auto-bezier flag | 8 | u32 | 1 normally, 0 after the spatial auto-bezier / continuous switches were turned off (changed together, cannot be separated) | 054 | E |
| segment length | 16 | f64 | arc length to the next key, updated lazily by After Effects (safe to ignore) | 052, 054 | E |
| ease slots | 24, 32, 40, 48 | 4 x f64 | two (speed, influence) pairs, exposed as stored: the ease preset puts speed 0 at 24 and 40 and influence 0.33 at 32 and 48; which pair is "in" and which "out" is unresolved | 052 | V (positions of speed and influence) |
| value | 56, 64, 72 | 3 x f64 | x, y, z in pixels | 055 (`[0,0,0]`, `[100,0,0]`, `[200,100,0]`), 056 | V |
| in tangent | 80, 88, 96 | 3 x f64 | spatial, relative offset from the value | 054 (key 2: `[-30, -60, 0]`) | V |
| out tangent | 104, 112, 120 | 3 x f64 | spatial, relative offset | 054 (key 1: `[30, 60, 0]`) | V |

Scalar record (48 bytes, Opacity): value as an `f64` fraction at 8 (0.0 and 1.0 in 058); bytes
16..47 are four `f64` that are zero for linear keys and are exposed raw. Marker record (16 bytes):
time, `03 03` hold bytes, 8 stale bytes (122, 123). Mask path record (64 bytes): the time and the
common fields only (093).

Test: `tests/values.rs`.

### 3.6 Expressions

| Field | Chunk path | Meaning | Evidence | Conf. |
|---|---|---|---|---|
| source | `tdbs/Utf8`, after `cdat` / `list` and before `tdum` | raw UTF-8 text, no length prefix, no NUL, no engine tag | 060 (`wiggle(1,10)` on Opacity) | V |
| flags | `tdb4` 76, 77, 120 | all 1 when present, all 0 otherwise | 060, 040 | V |
| static value | `cdat` | kept while an expression exists (Opacity 1.0) | 060 | V |

The expression engine is project-wide (`LIST ExEn/Utf8`, `javascript-1.0`). Test: `tests/values.rs`.

### 3.7 Effects (`tdgp[ADBE Effect Parade]/sspc`)

| Field | Chunk path | Offset | Type | Meaning | Evidence | Conf. |
|---|---|---|---|---|---|---|
| match name | the `tdmn` before the `sspc` | | text | `ADBE Slider Control`, `Pseudo/@@S1iPXtJFTHK4QaDbgJvLhw`, ... | 071, 073, 077, 080 | V |
| instance name | `sspc/fnam/Utf8` | | text | `Slider Control` | 071, 073, 077, 080 | V |
| parameter count | `sspc/parT/parn` | 0 | u32 | including the implicit input layer and the built-in entry (3 for a slider) | 071 | V |
| parameter match name | `parT/tdmn` | | text | `<effect match name>-NNNN`; the last is `ADBE Effect Built In Params` | 071 | V |
| parameter type | `parT/pard` | 12 | u32 | 0 layer, 2 fixed slider (16.16), 3 angle, 4 checkbox, 5 colour, 6 point, 7 popup, 9 no data, 10 float slider, 12 path | 071, 073, 077, 080 (slider 10, colour 5, popup 7; drop shadow 5, 2, 3, 2, 2, 4) | V |
| parameter name | `pard` | 16 | 32 ASCII | `Slider`, `Color`, `Menu`, `Shadow Color`, `Opacity`, `Direction`, `Distance`, `Softness`, `Shadow Only` | same | V |
| default | `pard` | 56 | by type | float slider: f64 (Slider 0); fixed slider and angle: 16.16 (drop shadow opacity 127.5, direction 135, distance 5, softness 0); popup: u32 index (1) | 071, 077, 080 | V |
| popup items | `parT/pdnm/Utf8` | | text | items separated by `\|` | 077 (`Item 1\|Item 2\|Item 3`) | V |
| stored value | `sspc/tdgp/<match>-NNNN` | | `tdbs` | present only for a parameter that differs from its default; `tdsb` 1, `cdat` as in 3.4 | 071 (77), 073 (colour) | V (float slider, colour) |

Test: `tests/effects.rs`.

### 3.8 Masks and bezier paths (`tdgp[ADBE Mask Parade]`)

| Field | Chunk path | Offset | Type | Meaning | Evidence | Conf. |
|---|---|---|---|---|---|---|
| inverted | `Mask Parade/mkif` | 0 | u8 | | 090 (0), 091 (1) | V |
| mode | `mkif` | 4 | u32 | 1 = Add, 2 = Subtract | 090, 091 | V |
| name | `tdgp[ADBE Mask Atom]/tdsn/Utf8` | | text | `Mask 1` | 090 | V |
| feather, opacity, expansion | `ADBE Mask Feather`, `ADBE Mask Opacity`, `ADBE Mask Offset` | | see 3.4 | stored only when not at the default | 090, 091 | V |
| path property | `om-s[ADBE Mask Shape]/tdbs` | | | animated: `tdb4` byte 68 = 1 and a `list` of 64-byte records (time and common fields only) | 090, 093 | V |
| path shapes | `om-s/omks/LIST shap` | | | one per keyframe, one for a static path | 090, 093 | V |
| box | `shap/shph` | 4, 8, 12, 16 | 4 x f32 | left, top, right, bottom **normalised to the layer size** for masks (560 / 1920, 340 / 1080, 1360 / 1920, 740 / 1080) | 090 | V |
| closed flag | `shph` | 20 | u8 | 1 in every sampled path (open paths are not sampled) | 090 | E |
| points | `shap/list` (`lhd3` record size 8) | | f32 x, y | three per vertex in the order **vertex i, out handle i, in handle i + 1**, normalised inside the box (u = (x - left) / (right - left)); the last record is the in handle of vertex 0 | 090 (rect), 093 (two rects) | V (without tangents), E (order with tangents) |

There is one `mkif` per parade in all samples (a single mask), so mode and inverted are reported for
the first mask only. Test: `tests/masks.rs`, with a synthetic check of the handle order.

### 3.9 Text documents (`btds[ADBE Text Document]/LIST btdk`)

`btdk` is not a chunk sequence but a PostScript-like dictionary (`/key value` pairs, `<< >>`, `[ ]`,
numbers, `true` / `false`, `/names`, strings as `(` + `fe ff` + UTF-16BE + `)` with `\` escapes). The
verified paths from the top level (keys `/98 /0 /1`):

| What | Path | Evidence | Conf. |
|---|---|---|---|
| text | `/1/1[0]/0/0`; a trailing `\r` ends the paragraph | 110 (`A`), 113 (`Hello`) | V |
| font size | `/1/1[0]/0/6/0[0]/0/0/6/1` (100 by default) | 110, 113 (60) | V |
| fill colour | `/1/1[0]/0/6/0[0]/0/0/6/53/0/1` = `[A R G B]` floats 0..1 | 110 (white), 113 (`[1 1 0.5 0]`) | V |
| font index and font list | style `/0` indexes `/0/1/0[i]/0/0/0` (a PostScript name) | 110, 113 (`TimesNewRomanPSMT`) | E |

Test: `tests/text_markers_footage.rs`.

### 3.10 Markers

`mrst[ADBE Marker]` holds a `tdbs` with a `list` of 16-byte records (the times; `tdb4` byte 12 is the
time scale) and one `LIST mrky/LIST Nmrd` per marker whose first `Utf8` child is the comment (four
more empty `Utf8` strings follow; their meaning is unresolved). Layer markers live in the layer's own
tree, composition markers in the `SecL` list named `Markers`.

| What | Evidence | Conf. |
|---|---|---|
| time (25 600 = 1 s) and comment | 122 (`CompMarker`), 123 (`LayerMarker`) | V |

Test: `tests/text_markers_footage.rs`.

### 3.11 What the tests cover

`tests/header.rs`, `values.rs`, `effects.rs`, `masks.rs` and `text_markers_footage.rs` compare the
decoded fields with the manifest values of the cases listed above; `typed.rs` checks the structural
relations on every property and every list of every fixture; `robustness.rs` runs the whole typed
view over damaged copies of the decoded fixtures (byte flips, truncation, damaged chunk bodies) and
over extreme values.

## 4. Not decoded

Fields that the corpus leaves open (**?**), only suggests (**H**), or shows without a manifest value to
assert against (**E**, not exposed), and areas that were not sampled. None of these is decoded by the
crate; the raw bytes stay reachable through the chunk tree.

**Layer header (`ldta`)**

- Bits `0x01`, `0x02`, `0x04` of byte 39 are set in every AV layer of the base and no case clears them (**?**): the video, audio and effects switches are presumably among them, so `enabled`, `audioEnabled` and `effectsActive` are not exposed. Same for quality Wireframe, track mattes, frame blending, time remapping and `autoOrient` (not sampled).
- Time stretch other than a whole multiplier (50 %, 33.3 %), blend modes other than Normal and Multiply, layer names longer than 31 characters (**?**).
- Bytes 56 (1 on AV, shape and text layers, 0 on camera and light; meaning ?), 96 and 108 (2 and 1 on AV layers), and the camera extra at 148 / 152 (a double that equals the 36 mm film size in points; **E** for the value, **H** for the meaning).

**Composition header (`cdta`, `PRin`)**

- Bytes 0, 16, 24, 32, 40, 48, 168, 176, 184, 196 and 200 (**?**): constants (1, 600, the timescale, 360, 128, 16) or values that flip irregularly between saves; bytes 24 and 48 equal the timescale (**E**).
- Whether the background colour is 8 or 16 bit per channel, non-integer frame rates (29.97, 23.976) and pixel aspect ratios (0.9091), the shutter phase, the start time / start frame, preserve-frame-rate and preserve-resolution switches, draft 3D, `Hide shy layers`, frame blending (not sampled).
- `PRin/prda` (12 bytes for Classic 3D, 52 for Advanced 3D: renderer parameters, **E**).
- Item `idta` byte 20, `idpc`, `head`, `nhed`, `nnhd`, `Ewst`, the `f*` records, `Gide`, `PTRE`, `LRdr` and `LSIf` bodies (not analysed).

**Properties and keyframes**

- `tdb4` bytes 4, 7 (3 on spatial properties, 1 on colours: **E**), 8..11, 16 (0.0001: **E**), 24 (the comp pixel aspect on spatial properties: **E**), 32..48 (1.0: **E**), 56..67 (a class pair: **?**), 72..78 except 76 and 77, 79 (**E**), 119; `tdsb` bits other than 0x1 and 0x2; `tdum` / `tduM` (**?**).
- Which of the two interpolation bytes is the incoming and which the outgoing side, and which of the two ease pairs is "in" and "out" (**?**: the hold case sets both sides, the ease preset is symmetric). The interpolation names are likely, not proven by a second source.
- The ease slots of the 48-byte scalar record for non-linear keys (only linear keys were sampled), and the layouts of the Scale (3-D non-spatial), Rotation, 2-D (mask feather, rect size), colour, text and shape keyframe records.
- The 8 stale bytes of the marker record and the bytes after the common fields of the mask path record (uninitialised memory).
- Expression enable / disable (which of the three flag bytes is the switch), multi-line and non-ASCII expressions, the ExtendScript engine.

**Effects**

- `pard` byte 48 (flags), 60 (popup choice count as a `u16`: **E**), 68 / 72 (point default: **E**), 104 / 108 (float slider valid minimum and maximum: **E**), 116 / 120 (slider range and default: **E**), 124, 128..147 (fixed slider ranges: **E**), and the stale memory after the parameter name. Minimum and maximum are therefore not exposed.
- The default of checkbox, point and colour parameters, how "None" versus a chosen layer is stored in a Layer Control (**?**), and whether built-in effects such as Gaussian Blur keep their current value in `pard` (the 25 of case 078 is the value that was set).
- Keyframed or expression-driven effect parameters, effects that are switched off, several effects on one layer, renamed effects, and the units of stored fixed-slider and angle values (not sampled).

**Masks, shapes**

- Whether `mkif` is stored once per mask (one `mkif` per parade in every sample), mask modes other than Add and Subtract, locked, colour, feather falloff, rotobezier (not sampled).
- Open paths (the byte at `shph` 20 is 1 in every sampled path), the sign and length of bezier tangents (no curved vertex was sampled; only the order of the three points per vertex is inferred), masks on a layer whose size differs from the composition (whether the box is normalised to the layer or the composition size).
- Shape layers (cases 100 to 105) are not decoded beyond the generic `ShapePath`: the group / contents tree (`ADBE Vector Group`, rectangle, ellipse, polystar, path, fill, stroke, trim, repeater) is only reachable as raw property groups. The path of a shape layer uses the same `shph` + point list with a box in pixels (105: box `-100, -100, 100, 100`).

**Text**

- Run lengths, stroke colour, paragraph (box) text, alignment, bold / italic, tracking, leading, per-range styles, animators and range selectors, non-BMP characters, and the roughly 700 other leaves of the dictionary.

**Camera, light, footage, markers**

- Camera and light properties (a camera's Position is stored as `0, 0, -2666.67` while After Effects reports `960, 540, -2666.67`: relative to the composition centre is only **H**), light options, intensity, colour, cone.
- Marker duration, chapter, URL and cue-point strings (`NmHd` and the order of the four empty `Utf8` strings), footage with audio, image sequences, footage interpretation, missing files, other alias JSON keys, time remapping.

## 5. Method

The format is established only from files we generate and compare, never from another program's
parser or from Adobe's code.

1. **Generate.** A base project is built once. For each case exactly one thing is changed through
   After Effects' own scripting interface (a layer switch, a composition property, a static value,
   a keyframe, an effect, a mask, a text edit), the project is saved as `NNN-name.aep`, the change
   is undone and the base state is re-checked before the next case. A manifest lists every case
   with the exact change and the values used; the same state is also saved as the XML flavour of
   the project, which names the chunks and can be used as a cross-check.
2. **Diff.** A walker (our own code) parses each file into the chunk tree, and each case is
   compared with the base and with its parent case: chunk path, size and the offsets and bytes
   that changed. Save noise (a save counter, time stamps, the growing XMP history) is filtered.
   Because one change produces a handful of chunk differences, the field that holds a value is
   found by the byte offsets, and checked by changing the value (`040` against `041`, `055`
   against `051`).
3. **State a hypothesis, then verify it.** A statement enters this document only when the manifest
   value and the decoded value agree in the case files (the table above names them) and the same
   relation holds on every other file where it applies. The tests in `crates/aep/tests/` are the
   verification script: they decode the fixtures and compare with the manifest values (layer
   switches, in / out / start times, composition size, frame rate, work area and background, Position
   `100, 200, 0`, Scale 50 %, key times 0 s, 1 s and 2.5 s, Slider 77, mask vertices, text and
   fill colour, marker times), and check the structural relations on every chunk of every fixture.
   A field with several cases that agree is marked V in section 3; a field seen only in the case
   diffs is marked E.
4. **Skip what is not verified.** Anything without an agreeing manifest value, or with a
   conflicting case, stays undecoded (section 4).

The corpus itself and the helper scripts live outside this repository. Only the 52 fixtures above
are committed, as the contributor's own test data (see `ATTRIBUTION.md`); the repository holds no
Adobe sample project, preset or asset.

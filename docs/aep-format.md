# After Effects project format (`.aep`)

> **Last reviewed:** 2026-10-13 · **Last updated:** 2026-10-13 · **Change:** new (container reader/writer `effectcraft-aep`, typed view of items, layers, properties and keyframes) · **Target:** Adobe After Effects 2026

What `crates/aep` ([`effectcraft-aep`](../crates/aep/README.md)) reads and writes, and the evidence
for each statement. Everything here was observed on project files we generated ourselves; nothing
comes from a third-party parser, specification or source. This is the container and a few
well-established fields, not a project importer: turning a parsed `.aep` into an EffectCraft
document is future work (see [gaps.md](gaps.md) G4 and [file-format-parity.md](file-format-parity.md)).

Evidence is cited as **case numbers**: the files `NNN-*.aep` of our synthetic corpus (78 files plus
an empty project, 79 in all), each made by one scripted change to a base project. The base project
`000-base` is one composition `C` (1920x1080, 25 fps, 5 s) holding one red solid layer `L` in the
folder `Solids`. Nine of these files are the test fixtures in
`crates/aep/tests/fixtures/` (`001`, `000`, `015`, `031`, `040`, `055`, `071`, `090`, `110`); the
tests named below run on them. Larger production projects were examined locally for statistics
only (chunk id counts, nesting depth, size relations); nothing from them is in the repository.

## 1. Container

| Fact | Evidence |
|---|---|
| The file starts with `RIFX`, a big-endian `u32` size and the form type `Egg!`; all integers in the structure are big-endian. | All 79 files. |
| The declared size is `4 + the chunks`; it ends before the end of the file. The bytes after it are an XMP packet (`<?xpacket begin` ... `<?xpacket end=...?>`, UTF-8 XML, 5.8 KB in `000`) that grows with every save. | All 79 files; test `header_and_trailer_facts`. |
| A chunk is a 4-byte id, a big-endian `u32` body size and the body; an odd-sized body is followed by one pad byte, always `0` in the corpus (8639 odd bodies). | Test `odd_sized_chunks_keep_their_pad_byte_even_when_not_zero` keeps any value. |
| The chunks of every file tile the container exactly: no leftover bytes, no chunk larger than its parent. | 79 of 79 files parse without a repair. |
| A `LIST` chunk's body is a 4-byte list type followed by chunks. List types seen include `Fold`, `Item`, `Layr`, `tdgp`, `tdbs`, `list`, `Pin ` and `FEE ` (the space is part of the type). | `000`; test `list_types_ending_in_a_space_are_kept`. |
| Lists can be empty (size 4, only the type): `Ewst`, `Pefl`, `CPPl`, `LItm`. | `000`; test `empty_list_and_list_shorter_than_its_type`. |
| Nesting: at most 13 levels in the corpus; larger production projects reached 23. | All 79 files; test `nesting_depth_is_below_the_limit`. |
| `LIST btdk` is not a chunk sequence: its body (7775 bytes in `110`) is text. A reader must keep it opaque. Only files with a text layer have it. | `110`, `111`, `112`, `113`, `114`; test `raw_text_lists_are_kept_opaque`. |
| Three non-`LIST` ids hold a chunk instead of data: `tdsn`, `pdnm`, `fnam`. Each holds exactly one `Utf8` chunk (a name or label, UTF-8 text, possibly empty). | `tdsn` 17 186 times, `fnam` 22, `pdnm` 14 over the corpus; test `nested_chunk_ids_hold_one_utf8_chunk`. |
| Ids can end in a space; the space is part of the id. In the corpus this happens for the list types `Pin ` and `FEE ` (larger production projects also have plain chunk ids that end in a space). All ids are 4 printable ASCII bytes. | `Pin `, `FEE ` in 78 of 79 files (all but `001`); tests `list_types_ending_in_a_space_are_kept`, `ids_ending_in_a_space_are_kept`. |
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

Only the rows below are decoded; each has a test in `crates/aep/tests/typed.rs` against the values
the cases were made with. Everything else is reachable as raw chunks.

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
| A `LIST list` is `lhd3` (52 bytes, 13 `u32`) and `ldat`. Word 0 is `0x00d00bee`, word 2 the record count, word 4 the record size, words 8 to 12 are zero; `len(ldat) = count * record size`; a list without records has no `ldat`. In the keyframe lists of properties, word 3 is `ceil(count / 4)` and word 7 the count rounded up to a multiple of 4 (not in the empty `Gide` lists). | The first four relations hold for every `list` of all nine fixtures (test `list_header_facts_hold_in_every_list_of_every_fixture`); word 3 and 7 on the keyframe lists of `050` (1 record) and `055` (3 records of 128 bytes, capacity 4). |
| A keyframe record starts with its time as a big-endian `i32` in the composition's time units. | `055` (0, 25 600, 64 000 for keys at 0 s, 1 s, 2.5 s), `058` (0 and 51 200 for keys at 0 s and 2 s). |
| A 128-byte record whose `u16` at byte 6 is 7 is a spatial (Position) key; its x, y, z are doubles at bytes 56, 64 and 72. | `050` (`100, 200, 0`), `055` (`0,0,0`, `100,0,0`, `200,100,0`). |

## 3. Observed, not decoded

Seen in the corpus and stable enough to name, but not interpreted by the crate (no test would be
honest about them yet):

- Keyframe records: the interpolation bytes (4 and 5) and flags (`051`, `052`, `053`), and the
  48-byte scalar record, where the value is the double at byte 8 (`058`: Opacity keys 0 and 1.0
  at 0 s and 2 s).
- The layer switches other than 3D, one bit of `ldta` each: shy and locked (byte 39, `012`,
  `013`), solo (byte 38, `014`), motion blur (byte 39, `017`), adjustment layer (byte 38, `025`),
  guide layer (byte 37, `026`).
- The rest of `cdta` and `ldta` (size, duration, in and out points: cases `030`, `032`, `019` to
  `021`), masks (`090` to `093`), text documents (`110` to `114`), shape layers (`100` to `105`),
  expressions (`060`, `061`: the source looks like a `Utf8` child of the `tdbs`), markers
  (`122`, `123`), cameras and lights (`120`, `121`), precomps (`124`) and footage (`125`, `126`).
- Anchor Point is stored as a fraction of the layer size, not in pixels (`044`: `10, 20` is
  `10/1920, 20/1080`), unlike Position (`040`).
- The 4-byte item id chunks `iide` and `sfid` hold `0d 00 00 00` and `19 00 00 00` in `000`,
  which looks little-endian, unlike the rest of the structure.

## 4. Method

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
   verification script: they decode the fixtures and compare with the manifest values (Position
   `100, 200, 0`, key times 0 s, 1 s and 2.5 s, Slider 77, 25 and 30 fps), and check the
   structural relations on every chunk of every fixture.
4. **Skip what is not verified.** Anything without an agreeing manifest value, or with a
   conflicting case, stays undecoded (section 3).

The corpus itself and the helper scripts live outside this repository. Only the nine fixtures above
are committed, as the contributor's own test data (see `ATTRIBUTION.md`); the repository holds no
Adobe sample project, preset or asset.

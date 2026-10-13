# effectcraft-aep

Reads and writes the container of After Effects project files (`.aep`) as a lossless chunk tree.

An `.aep` is a `RIFX` container (big-endian RIFF) with form type `Egg!`: chunks of a 4-byte id, a
big-endian `u32` size and a body padded to an even length; `LIST` chunks start with a 4-byte list
type followed by chunks. `parse` builds the tree, `write` serialises it, and for every input that
parses, `write(parse(x)) == x` byte for byte (container and chunk sizes are recomputed from the
tree; pad bytes, odd bodies and the bytes after the container, an XMP packet, are kept).

Bodies that look like lists but are not chunk sequences (for example `LIST btdk`, raw text) stay
opaque rather than failing the parse. Input is untrusted: sizes are checked against the remaining
input, nesting and chunk count are bounded (`Limits`), nothing is allocated from a declared size,
and truncated or damaged files return `Err` (or, inside the container, an opaque body).

No field of any chunk body is decoded by the container layer. What is known about the content, and how
it was found, is in [`docs/aep-format.md`](../../docs/aep-format.md).

## Clean room

Everything here comes from our own observations: project files that we generated ourselves with
After Effects, one change per file, compared byte by byte. No third-party parser, specification
or source was read, and no Adobe asset is used. The test fixtures in `tests/fixtures/` are such
generated files (own test data, attributed in `ATTRIBUTION.md`).

# Proposal: format version 2, plain text (M5)

Status: **approved by the owner on 2026-10-09, all four recommendations (DEC-031), and built.**
The format as built is specified in `docs/file-format.md`; this page keeps the reasoning.

## Why

Version 1 is a zip holding pretty-printed JSON. Git sees a binary file, so a diff says only
"binary files differ" and every merge conflicts. Inside the zip:

- **Volatile data:** `generator` changes with every release.
- **Computed numbers that drift:** fingerprints such as `"y": 29.999999999999996` and
  `"area": 4769.097335529232`, and solved sketch positions, all change in the last digits when
  they are recomputed.
- **Verbose layout:** the M2 enclosure (13 features) takes 760 lines and 18 KB, mostly one sketch
  point spread over 12 lines.
- **A stale spec:** `docs/file-format.md` lists 3 of the 14 feature types. Fillets, holes,
  patterns, work features, ribs, parameters and the End of Part marker were added within
  version 1 without being written down.

## Proposal

1. **One plain UTF-8 text file per document, no zip.** Same extensions (`.tenon`, and with
   question 3 `.tenonasm` and `.tenondrw`). Files are written with LF line endings and a final
   newline; files with CRLF line endings (Git's `autocrlf` on Windows) are read too.
   - Nothing else goes in the file. The reserved `cache/` and `thumbnail.png` were never used.
   - A future B-rep cache would live in the user's cache folder, keyed by a hash of the file, and
     never be authoritative.
2. **TOML syntax** (recommended in question 2), written by Tenon's own writer with a fixed layout,
   read with the `toml` parser (MIT/Apache, already a dependency of a dependency).
   - Each feature is a `[[feature]]` table, one field per line.
   - Lists of records (sketch entities, constraints, fillet edges, parameters) put one record per
     line, each ending in a comma. Adding a record never touches its neighbours' lines.
   - Small nested values are inline tables on one line.
3. **Stable output.** Saving the same document twice gives byte-identical files on every OS,
   checked in CI against checked-in expected files:
   - fields come in a fixed order (the model's declaration order); unknown top-level fields
     follow, sorted;
   - no `generator`, no timestamps, no absolute paths (assemblies and drawings already store
     relative ones);
   - values the user typed (dimensions, distances, equations) are written exactly (the shortest
     text that reads back as the same number);
   - **computed** values (solved sketch positions and reference fingerprints) are rounded to
     1e-9 mm, a hundredth of the kernel's smallest tolerance (1e-7 mm). A recompute then no longer
     shows up as a diff. `-0.0` is written as `0.0`.
4. **What is kept from version 1:**
   - integer ids that are never reused, with their counters (`next_feature` etc.);
   - every field written explicitly, except empty optional ones (TOML has no `null`). There are
     no implicit defaults, so a changed default can never change an old file;
   - the size limits, validation and "damaged" or "made by a newer Tenon" errors. Text errors
     add a line and column ("line 41, column 12: expected a number").
5. **Versions and migrations.**
   - Version 2 files start with `format` and `version`.
   - Every version-1 zip opens forever: it is read as version 1, migrated in memory, and saved as
     version 2. Each later version adds one migration step `n -> n+1`, tested.
   - Older Tenon builds cannot read version 2. So the first time a version-1 file is saved over,
     the original is kept once beside it as `name.v1.tenon` (question 4).
6. **`tenon diff a.tenon b.tenon`:** what changed, by feature and by parameter. Either file may
   be version 1 or 2. Plain text output, `--json` for tools, and exit code 0 for same, 1 for
   different, 2 for an error. It also works as a Git diff tool. Example:

   ```text
   Parameters
     ~ H  30 mm -> 35 mm  (outside height)
   Features
     ~ Extrusion1 [2]  extent distance 30 -> 35   (from H)
     ~ Sketch1 [1]     dimension d1: 60 -> 62; 2 points moved
     + Fillet2 [14]    radius 1, 4 edges
     - Hole2 [13]
     > Hole1 [10]      moved after Rectangular Pattern1
   ```

   A value that changed because a parameter changed names it ("from H"), so a parameter edit
   reads as one change, not many.
7. **Merging.** Edits to different features or parameters merge cleanly in Git. When two
   branches both add a feature, both new features take the same id. The merged file is then
   refused on open, with both features named ("two features have id 14"). Renumbering such a
   merge automatically (`tenon merge`) is listed as a gap, not part of M5.

## Example: the start of the M2 enclosure (an excerpt: some records left out)

```toml
# Tenon part, format version 2 (docs/file-format.md)
format = "tenon"
version = 2
name = "Enclosure"
next_feature = 14

[parameters]
next = 18
user = [
  { name = "L", equation = "80 mm", unit = "mm", comment = "outside length" },
  { name = "W", equation = "60 mm", unit = "mm", comment = "outside width" },
  { name = "H", equation = "30 mm", unit = "mm", comment = "outside height" },
  { name = "t", equation = "2 mm", unit = "mm", comment = "wall thickness" },
]
model = [
  { name = "d0", target = { kind = "dimension", sketch = 1, constraint = 7 }, equation = "L" },
  { name = "d1", target = { kind = "dimension", sketch = 1, constraint = 8 }, equation = "W" },
  { name = "d2", target = { kind = "feature", feature = 2, field = "/extent/distance" }, equation = "H" },
  { name = "d3", target = { kind = "feature", feature = 3, field = "/radius" } },
]

[[feature]]
id = 1
name = "Sketch1"
suppressed = false
type = "sketch"
plane = { origin = "XY" }
next_entity = 9
next_constraint = 8
entities = [
  { id = 1, type = "point", pos = { x = 0.0, y = 0.0 }, construction = true },
  { id = 3, type = "point", pos = { x = 80.0, y = 0.0 }, construction = false },
  { id = 6, type = "line", start = 2, end = 3, construction = false },
]
constraints = [
  { id = 1, type = "fix", point = 1 },
  { id = 2, type = "horizontal", line = 6 },
  { id = 7, type = "length", line = 6, value = 80.0 },
]

[[feature]]
id = 2
name = "Extrusion1"
suppressed = false
type = "extrude"
sketch = 1
regions = "default"
extent = { distance = 30.0 }
reverse = false
operation = "join"

[[feature]]
id = 3
name = "Fillet1"
suppressed = false
type = "fillet"
edges = [
  { faces = [{ type = "side", feature = 2, curve = 6 }, { type = "side", feature = 2, curve = 7 }], fingerprint = { mid = { x = 80.0, y = 0.0, z = 15.0 }, length = 30.0 } },
  { faces = [{ type = "side", feature = 2, curve = 8 }, { type = "side", feature = 2, curve = 7 }], fingerprint = { mid = { x = 80.0, y = 60.0, z = 15.0 }, length = 30.0 } },
]
radius = 6.0
```

The same feature in the alternative, canonical JSON (question 2):

```json
{"id": 2, "name": "Extrusion1", "suppressed": false, "type": "extrude", "sketch": 1, "regions": "default", "extent": {"distance": 30.0}, "reverse": false, "operation": "join"},
```

## Questions for the owner

1. **Container:** one plain text file per document (recommended), instead of a zip?
2. **Syntax:**
   - **TOML (recommended):** one field per line and no commas between features, so diffs are
     one line per change and appends never conflict. It is familiar from config files.
   - **Canonical JSON, one record per line:** cheaper to build and more universal, but long lines
     per feature, and a comma joins each record to the next, which makes appends conflict in
     Git.
3. **Scope:** move assemblies (`.tenonasm`) and drawings (`.tenondrw`) to version 2 by the same
   rules, so all three behave alike (recommended)? Or `.tenon` only, as the plan says?
4. **Backups:** keep `name.v1.tenon` the first time a version-1 file is saved as version 2
   (recommended)? Or no backup?

## Tests (with the implementation)

- **Round trip:** every example file opens, saves and opens again equal. A second save is
  byte-identical. The geometry after regenerating (volume, area, bounding box) equals that from
  the version-1 file.
- **Migrations:** frozen copies of every version-1 file in `examples/` (parts from M1 to M4,
  assemblies, drawings) migrate and regenerate. They stay as fixtures for every later version.
- **Stability:** checked-in expected version-2 files for the examples, compared byte for byte on
  Linux, macOS and Windows.
- **Damaged text:** truncated files, wrong types, duplicate ids, unknown types, CRLF and odd names
  (quotes, backslashes, newlines, non-Latin letters) give errors with a line number, or read
  correctly. They never crash.
- **Merge:** a parameter changed on one branch and a fillet added on another merge cleanly with
  `git merge-file`. The result opens and regenerates.
- **`tenon diff`:** each kind of change in the example above, on version-1 and version-2 inputs.

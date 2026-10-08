# Native project format

**Status: not yet defined.** The format arrives with save/load in M1. Until then Tenon reads and
writes only open exchange formats (STEP via the kernel, STL).

Approved direction (docs/decisions.md, DEC-004):

- **Container:** a zip file with the extension `.tenon`.
- **`project.json`:**
  - the source of truth: parameters, sketches, the feature tree, persistent references,
    appearance and document metadata;
  - carries a top-level `"format": "tenon"` and an integer `"version"`.
- **`cache/*.brep` (optional):** kernel B-rep caches, keyed by backend name and version, used only
  to open files faster. They are always regenerable from `project.json` and are ignored when the
  backend differs.
- **`thumbnail.png` (optional).**

Rules the M1 schema must follow:

- Never store kernel handles or sub-shape indices; store persistent references
  (docs/persistent-naming.md).
- Units are millimetres and radians in the file; display units are a document preference.
- Every schema change bumps `version` and comes with a migration from the previous version and a
  round-trip test.
- Unknown fields are preserved on save.
- Hostile input is rejected with an error, never a panic, and with caps on sizes and nesting.
- Changing the container (zip/JSON) after M1 needs the project owner's approval.

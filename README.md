# Tenon

Tenon is a free, open-source, cross-platform parametric 3D CAD application written in Rust. The
workflow is the familiar one for mechanical design: sketch, constrain, build features, assemble,
then document in drawings.

**Status: early development (milestone M0, foundations).** There is no modelling UI yet. What works
today is listed, with the tests that prove it, in [ROADMAP.md](ROADMAP.md).

## Design

- **Kernel behind a trait.** All geometry goes through the backend-neutral `Kernel` trait
  (`crates/kernel`). The first backend binds [OpenCASCADE Technology](https://dev.opencascade.org)
  8 through a small C++ shim (`crates/kernel-occt`), the only crate allowed to use `unsafe`. A
  Rust-native kernel can replace it later, operation by operation (milestone M6).
- **Persistent naming from day one.** Every kernel operation reports where faces and edges came
  from, so features can refer to geometry in a way that survives upstream edits
  ([docs/persistent-naming.md](docs/persistent-naming.md)).
- **Everything is a command.** Every UI action maps to a named command that scripts, the CLI and
  the MCP server can drive as well.
- **Strict layering.** `cargo xtask layers` enforces the crate graph: nothing below `ui` knows
  about egui, nothing above `kernel` knows about OpenCASCADE.

See [docs/architecture.md](docs/architecture.md) and [docs/plan.md](docs/plan.md).

## Building

You need Rust (stable, 1.90 or later), a C++17 compiler (MSVC 2022 on Windows, Xcode command line
tools on macOS, gcc or clang on Linux) and [pixi](https://pixi.sh), which provides OpenCASCADE.
Step-by-step instructions per platform are in [docs/setup.md](docs/setup.md).

```sh
pixi install                       # once: OpenCASCADE 8 into .pixi/
pixi run cargo test --workspace    # build and test
pixi run cargo xtask ci            # the full gate CI runs
pixi run cargo run -p tenon-cli -- demo m0 --out out
```

## Origins

Tenon is a fork of [CADCraft](https://github.com/storytold/cadcraft) (MIT OR Apache-2.0) at
commit `14e143b`. It keeps CADCraft's 2D geometry, DXF reader/writer and workspace tooling, and
will port its sketch constraint solver, command registry and MCP server. CADCraft's AutoCAD-style
drafting stack and the ArtCraft brand were removed. See [NOTICE](NOTICE).

## License

Tenon's own code is licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option. OpenCASCADE Technology is a separate library under
LGPL-2.1 (with the Open CASCADE exception), linked dynamically; see [NOTICE](NOTICE) for how to
obtain its source and replace it.

## Trademarks

Tenon is an independent project. It is not affiliated with, sponsored by or endorsed by
Autodesk, Inc. or any other CAD vendor. Autodesk, Inventor and AutoCAD are registered trademarks
of Autodesk, Inc. All other trademarks are the property of their respective owners. Tenon does not
read or write any vendor's proprietary file formats; it uses open formats (STEP, STL, 3MF, DXF)
and its own documented project format.

# Working on Tenon

Rules for humans and coding agents. The plan and milestone order are in `docs/plan.md`; decisions
are logged in `docs/decisions.md`; what actually works is in `ROADMAP.md`.

## Build and check

- Native dependencies come from pixi. Prefix cargo commands with `pixi run` (it puts OpenCASCADE
  on the include, link and runtime library paths). Setup per OS: `docs/setup.md`.
- Before every commit: `pixi run cargo xtask ci` (fmt, clippy `-D warnings`, tests, assets,
  layers, unsafe audit, wasm). Commit in small, reviewable steps.

## Never crash

- No `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!` or `unimplemented!` in non-test
  code (workspace clippy lints deny them; `clippy.toml` allows them in tests).
- Errors are `Result`s. Kernel operations return `KernelError`; nothing panics across the `Kernel`
  trait boundary and no C++ exception crosses the FFI boundary (the shim catches everything).
- Treat numbers from files, commands and MCP/CLI parameters as hostile: validate finiteness and
  ranges before they reach the kernel. Every crash fix gets a regression test.

## Architecture

- Layering is enforced by `cargo xtask layers` (table in `xtask/src/layers.rs`, explained in
  `docs/architecture.md`). Nothing below `ui` may depend on egui/eframe/winit; nothing except
  `render` and above may depend on wgpu; only `kernel-occt` may use FFI crates (`cxx`, `cc`).
- Library crates use kernel backends only as dev-dependencies; they talk to `dyn Kernel`.
- `unsafe` is allowed only in `crates/kernel-occt`. Every other crate root has
  `#![forbid(unsafe_code)]`; `cargo xtask unsafe-audit` checks both.
- Tolerances come from `tenon_geom::tol`. Do not scatter new epsilons.
- Features never store raw kernel indices; they store persistent references
  (`docs/persistent-naming.md`).
- Every user action is a named command usable from the UI, CLI, scripts and MCP.

## Honesty

- `ROADMAP.md` lists every feature as done / partial / missing. Mark a feature done only when a
  test proves it, and name the test.
- Report failures as failures. Do not weaken a test to make it pass.

## Clean room and legal

- Imitate workflow and general layout of established CAD tools only. Do not copy any vendor's
  icons, artwork, strings, sounds or documentation text, and do not use vendor product names in
  the product (say "orientation cube", "radial menu", "End of history").
- Do not read or write proprietary formats (`.ipt`, `.iam`, `.idw`, `.dwg` as a brand).
- Never commit screenshots of other vendors' software.
- Every bundled asset needs a row in `ATTRIBUTION.md` (`cargo xtask assets`).
- Our code is MIT OR Apache-2.0. OpenCASCADE (LGPL-2.1) stays dynamically linked; record any new
  third-party licence in `NOTICE`.

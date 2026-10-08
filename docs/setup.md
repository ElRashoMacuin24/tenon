# Setting up a development machine

Tenon needs:

- **Rust**, stable 1.90 or later (edition 2024), with `rustfmt`, `clippy` and the
  `wasm32-unknown-unknown` target;
- **a C++17 compiler** for the OpenCASCADE shim: MSVC 2022 on Windows, Xcode command line tools
  on macOS, gcc or clang on Linux;
- **[pixi](https://pixi.sh)**, which installs OpenCASCADE Technology 8 from conda-forge into
  `.pixi/` (pinned by `pixi.lock`);
- **git**.

Run cargo through `pixi run` so the OpenCASCADE headers, libraries and runtime library path are
set: `pixi run cargo test --workspace`. Alternatively open a pixi shell (`pixi shell`) and use
cargo normally inside it.

## Windows 11

```powershell
winget install --id Git.Git -e
winget install --id prefix-dev.pixi -e
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install --id Rustlang.Rustup -e
rustup target add wasm32-unknown-unknown
```

Install the Build Tools before Rust so rustup finds MSVC. Open a new terminal afterwards so the
new PATH entries apply. Then, in the repository:

```powershell
pixi install
pixi run cargo xtask ci
pixi run cargo run -p tenon-cli -- demo m0 --out out
cargo run -p tenon
```

The desktop app (`tenon`) does not link OpenCASCADE yet, so it runs without pixi. Anything that
links the kernel (`tenon-cli`, kernel tests) needs `pixi run`, because Windows finds the OCCT DLLs
through PATH. Without it, programs exit with `STATUS_DLL_NOT_FOUND` (0xc0000135).

### Smart App Control

If Windows **Smart App Control** is on (Windows Security > App & browser control > Smart App
Control), Windows judges every freshly compiled, unsigned `.exe` and `.dll` by reputation and may
refuse to run or load some of them. On the development machine used for M0 it blocked:

- a unit-test executable: cargo reports `An Application Control policy has blocked this file.
  (os error 4551)`;
- a proc-macro DLL loaded by rustc: the build fails with `can't find crate for ...`.

The block is visible in Event Viewer under *Applications and Services Logs > Microsoft > Windows
> CodeIntegrity > Operational* (events 3033, 3077, 3118). Which file gets blocked is not
predictable, so new dependencies can hit it at any time.

Options:

1. **Turn Smart App Control off** on the development machine. This is common for machines that
   compile code, but it is a security setting, so decide for yourself. On some Windows versions it
   cannot be turned back on without resetting Windows.
2. Work around individual blocks. The repository currently does two things: kernel-occt has no
   unit-test executable (`[lib] test = false`, all its tests are integration tests), and
   `Cargo.toml` overrides the profile of `zerocopy-derive` so the DLL gets a new hash. These are
   workarounds, not fixes.
3. Build inside WSL2 (Linux binaries are not subject to Smart App Control).

## macOS

```sh
xcode-select --install
curl https://sh.rustup.rs -sSf | sh
rustup target add wasm32-unknown-unknown
curl -fsSL https://pixi.sh/install.sh | sh
pixi install && pixi run cargo xtask ci
```

## Linux (Debian/Ubuntu)

```sh
sudo apt-get install build-essential git libxkbcommon-dev libwayland-dev \
  libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
curl https://sh.rustup.rs -sSf | sh
rustup target add wasm32-unknown-unknown
curl -fsSL https://pixi.sh/install.sh | sh
pixi install && pixi run cargo xtask ci
```

pixi sets `LD_LIBRARY_PATH` (Linux) and `DYLD_FALLBACK_LIBRARY_PATH` (macOS) to the environment's
`lib` directory; see `pixi.toml`.

## Using your own OpenCASCADE

Set `OCCT_ROOT` to the install prefix of an OCCT 8.0.x build (the directory containing
`include/opencascade` and `lib`) and make its shared libraries findable at run time. The build
script checks the major version and fails with a clear message otherwise.

**Status of other platforms:** Windows is verified locally. The Linux and macOS steps and the
GitHub Actions workflow (`.github/workflows/ci.yml`) are written but have not run yet; the first
push to GitHub will verify them.

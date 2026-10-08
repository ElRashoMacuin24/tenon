# M0 demo: L-bracket with three holes

Built by `tenon-cli` using only the M0 kernel operations (boxes, cylinders, booleans) through the
`Kernel` trait and the OpenCASCADE backend:

- base plate 60 x 40 x 8 mm, upright 60 x 8 x 30 mm (union);
- two 8 mm holes through the base, one 10 mm hole through the upright (one cut with three tools).

Expected volume: 60*40*8 + 60*8*30 - 2*pi*4^2*8 - pi*5^2*8 = 32167.434 mm^3. The CLI prints the
measured volume and the relative error; `apps/tenon-cli/tests/cli.rs` checks it and reads the STEP
file back.

Regenerate (from the repository root):

```sh
pixi run cargo run -p tenon-cli -- demo m0 --out examples/m0-bracket
pixi run cargo run -p tenon-cli -- info examples/m0-bracket/bracket.step
```

Files: `bracket.step` (AP214, millimetres) and `bracket.stl` (binary, millimetres). Open them in
any CAD or mesh viewer. The STEP header contains a timestamp, so it changes on every run.

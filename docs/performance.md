# Performance budgets

Budgets are checked in CI on every platform. They are set loosely (tens of times the measured
time) so slow shared runners do not flake; they catch order-of-magnitude regressions. Tighter
tracking with benchmarks comes later.

| Budget | Limit | Measured (Windows 11, desktop CPU) | Test |
|---|---|---|---|
| Regenerate the M1 demo bracket (6 features, 3 booleans) from scratch | 500 ms | 6 ms | `apps/tenon-cli/tests/m1.rs` `m1_demo_part_regenerates_and_tessellates_within_budget` |
| Tessellate and measure it for display | 500 ms | 6 ms | same |

Each figure is the median of five runs on a fresh kernel. The test prints the times (`cargo test
-p tenon-cli --test m1 -- --nocapture`).

## UI thread

The interactive app never regenerates on the UI thread: the workbench sends the document to a
worker thread (`tenon_model::worker`), keeps drawing the last result, and swaps in the new scene
when it arrives. A newer document cancels the regeneration in progress (between kernel
operations), and STEP export also runs on the worker. `crates/model/tests/model.rs`
(`worker_regenerates_off_thread_and_reports_the_latest`) checks that only the latest revision is
reported; cancellation itself has no dedicated test yet. A UI frame-time budget is planned for
M2, when sketches get large enough to matter.

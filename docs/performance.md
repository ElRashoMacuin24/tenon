# Performance budgets

Budgets are checked in CI on every platform. They are set loosely (tens of times the measured
time) so slow shared runners do not flake; they catch order-of-magnitude regressions. Tighter
tracking with benchmarks comes later.

| Budget | Limit | Measured (Windows 11, desktop CPU) | Test |
|---|---|---|---|
| Regenerate the M1 demo bracket (6 features, 3 booleans) from scratch | 500 ms | 6 ms | `apps/tenon-cli/tests/m1.rs` `m1_demo_part_regenerates_and_tessellates_within_budget` |
| Tessellate and measure it for display | 500 ms | 6 ms | same |
| UI work per frame on a 40-feature part (layout, picking, browser; CPU side, no GPU drawing) | 16 ms | 0.08 ms idle, 0.85 ms with the pointer moving (release); 0.11 / 2.29 ms (CI's dev profile) | `crates/ui/src/tests.rs` `frame_time_stays_within_budget` |

Each figure is the median of five runs on a fresh kernel. The test prints the times (`cargo test
-p tenon-cli --test m1 -- --nocapture`).

## UI thread

The interactive app never regenerates on the UI thread: the workbench sends the document to a
worker thread (`tenon_model::worker`), keeps drawing the last result, and swaps in the new scene
when it arrives. A newer document cancels the regeneration in progress (between kernel
operations), and STEP export also runs on the worker. `crates/model/tests/model.rs`
(`worker_regenerates_off_thread_and_reports_the_latest`) checks that only the latest revision is
reported; cancellation itself has no dedicated test yet. The UI thread does not clone or compare
the document every frame: it keys the shown geometry on the document revision and the open panel.

## Incremental regeneration

An edit recomputes from the first feature it changes, not from the start (docs/architecture.md,
"Regeneration"). Timing, not a budget (`cargo test --release -p tenon-model --test m2
regeneration_speed -- --ignored --nocapture`):

| Part | From scratch | Resumed at the edited feature |
|---|---|---|
| 42 features (block, 19 hole sketches and 19 holes, a boss); editing the boss height | 166 ms | 9.3 ms (18x) |

`regeneration_resumes_from_the_feature_being_edited` checks that the result is the same as a
regeneration from scratch (volumes, face names, statuses) and that no kernel shape leaks.

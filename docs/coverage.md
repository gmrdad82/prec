# Coverage

prec measures its test coverage with [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) over cargo-nextest. There is no fixed threshold: each change that adds code adds its tests, coverage is reported before and after, and the number only goes up.

## How to measure

The unit tests (no Chromium, no ffmpeg):

```
CARGO_LLVM_COV_TARGET_DIR=target/cov-unit cargo llvm-cov nextest --locked --workspace --bins --no-tests=pass --no-report
CARGO_LLVM_COV_TARGET_DIR=target/cov-unit cargo llvm-cov report --summary-only
```

The whole suite, the integration tests under `tests/` included (they drive the `prec` binary through headless Chromium and check masters with ffmpeg, and the binary they run is instrumented too):

```
CARGO_LLVM_COV_TARGET_DIR=target/cov-full cargo llvm-cov nextest --locked --workspace --no-tests=pass --no-fail-fast --no-report
CARGO_LLVM_COV_TARGET_DIR=target/cov-full cargo llvm-cov report --summary-only
```

Add `--json --output-path <file>` to the report for the per-file numbers, `--lcov --output-path <file>` for each line's count, or `--html` to see the uncovered lines.

Lines counts every instrumented line of a file, its `#[cfg(test)] mod tests` included, as the baseline did. Code lines counts only the lines above that module, read from the lcov report, so the tests' own lines don't lift the figure.

An integration test that clears the environment of the `prec` it runs keeps `LLVM_PROFILE_FILE`; without it the instrumented binary writes its profile into the working tree and the report never counts that run.

## Now (2026-10-22, after the command line, recording and rendering tests)

Unit tests (275): **79.4% of lines** (10,072 of 12,679), 73.5% of functions, 78.5% of regions; **66.6% of code lines** (4,596 of 6,906).

| File | Lines | Covered | Lines % | Functions % | Code lines | Code % | Lines % at v1.4.6 |
|---|---:|---:|---:|---:|---:|---:|---:|
| `src/home.rs` | 119 | 0 | 0.0% | 0.0% | 107 | 0.0% | 0.0% |
| `src/doctor.rs` | 166 | 68 | 41.0% | 72.7% | 115 | 19.1% | 0.0% |
| `src/config.rs` | 99 | 50 | 50.5% | 40.0% | 67 | 41.8% | 47.5% |
| `src/main.rs` | 551 | 287 | 52.1% | 45.2% | 313 | 20.8% | 5.3% |
| `src/render.rs` | 1,668 | 932 | 55.9% | 52.0% | 1023 | 34.3% | 32.0% |
| `src/replay.rs` | 418 | 263 | 62.9% | 69.8% | 268 | 45.9% | 8.8% |
| `src/paths.rs` | 600 | 405 | 67.5% | 65.7% | 408 | 57.6% | 64.0% |
| `src/cdp.rs` | 539 | 410 | 76.1% | 74.2% | 301 | 61.1% | 36.6% |
| `src/jobs.rs` | 106 | 81 | 76.4% | 72.7% | 103 | 76.7% | 17.9% |
| `src/record.rs` | 1,086 | 835 | 76.9% | 78.9% | 586 | 60.1% | 57.6% |
| `src/private.rs` | 305 | 243 | 79.7% | 65.9% | 171 | 70.2% | 79.7% |
| `src/jobs/env.rs` | 35 | 29 | 82.9% | 66.7% | 32 | 90.6% | — |
| `src/cache.rs` | 1,043 | 874 | 83.8% | 75.2% | 518 | 71.2% | 83.8% |
| `src/app.rs` | 890 | 752 | 84.5% | 78.0% | 446 | 72.6% | 41.4% |
| `src/jobs/util.rs` | 77 | 66 | 85.7% | 85.7% | 75 | 86.7% | — |
| `src/jobs/fit.rs` | 109 | 94 | 86.2% | 100.0% | 107 | 86.0% | — |
| `src/font.rs` | 152 | 138 | 90.8% | 66.7% | 39 | 71.8% | 0.0% |
| `src/archive.rs` | 1,637 | 1,518 | 92.7% | 71.3% | 713 | 89.1% | 91.9% |
| `src/encode.rs` | 633 | 607 | 95.9% | 77.8% | 300 | 99.0% | 76.7% |
| `src/theme.rs` | 428 | 415 | 97.0% | 89.8% | 243 | 96.7% | 31.3% |
| `src/jobs/job.rs` | 234 | 228 | 97.4% | 96.2% | 230 | 97.8% | — |
| `src/jobs/rate.rs` | 47 | 46 | 97.9% | 100.0% | 45 | 97.8% | — |
| `src/compose.rs` | 511 | 508 | 99.4% | 97.9% | 245 | 99.2% | 64.3% |
| `src/mask.rs` | 282 | 281 | 99.6% | 95.7% | 100 | 100.0% | 96.8% |
| `src/tape.rs` | 646 | 644 | 99.7% | 100.0% | 240 | 100.0% | 80.9% |
| `src/fps.rs` | 218 | 218 | 100.0% | 100.0% | 73 | 100.0% | 98.7% |
| `src/jobs/host.rs` | 80 | 80 | 100.0% | 100.0% | 38 | 100.0% | — |

Whole suite (350 tests, unit and integration): **95.4% of lines** (12,097 of 12,679), 87.7% of functions, 94.3% of regions; **94.5% of code lines** (6,526 of 6,906).

| File | Lines | Covered | Lines % | Functions % | Code lines | Code % | Lines % at v1.4.6 |
|---|---:|---:|---:|---:|---:|---:|---:|
| `src/home.rs` | 119 | 88 | 73.9% | 66.7% | 107 | 77.6% | 73.9% |
| `src/config.rs` | 99 | 75 | 75.8% | 68.0% | 67 | 73.1% | 62.6% |
| `src/jobs/fit.rs` | 109 | 94 | 86.2% | 100.0% | 107 | 86.0% | — |
| `src/paths.rs` | 600 | 528 | 88.0% | 83.8% | 408 | 87.0% | 77.7% |
| `src/cache.rs` | 1,043 | 950 | 91.1% | 85.3% | 518 | 85.3% | 85.2% |
| `src/main.rs` | 551 | 518 | 94.0% | 87.1% | 313 | 92.7% | 80.9% |
| `src/archive.rs` | 1,637 | 1,547 | 94.5% | 74.9% | 713 | 93.0% | 94.4% |
| `src/jobs/util.rs` | 77 | 73 | 94.8% | 100.0% | 75 | 97.3% | — |
| `src/cdp.rs` | 539 | 512 | 95.0% | 90.3% | 301 | 93.0% | 84.9% |
| `src/render.rs` | 1,668 | 1,594 | 95.6% | 88.2% | 1023 | 94.7% | 83.7% |
| `src/private.rs` | 305 | 292 | 95.7% | 90.9% | 171 | 95.3% | 95.1% |
| `src/encode.rs` | 633 | 610 | 96.4% | 78.8% | 300 | 100.0% | 85.1% |
| `src/theme.rs` | 428 | 415 | 97.0% | 89.8% | 243 | 96.7% | 46.8% |
| `src/app.rs` | 890 | 863 | 97.0% | 93.9% | 446 | 98.4% | 69.5% |
| `src/replay.rs` | 418 | 406 | 97.1% | 86.0% | 268 | 97.8% | 86.1% |
| `src/jobs/env.rs` | 35 | 34 | 97.1% | 100.0% | 32 | 100.0% | — |
| `src/jobs/job.rs` | 234 | 228 | 97.4% | 96.2% | 230 | 97.8% | — |
| `src/jobs/rate.rs` | 47 | 46 | 97.9% | 100.0% | 45 | 97.8% | — |
| `src/font.rs` | 152 | 150 | 98.7% | 86.7% | 39 | 100.0% | 87.5% |
| `src/doctor.rs` | 166 | 164 | 98.8% | 95.5% | 115 | 99.1% | 0.0% |
| `src/record.rs` | 1,086 | 1,073 | 98.8% | 93.6% | 586 | 99.0% | 85.3% |
| `src/compose.rs` | 511 | 508 | 99.4% | 97.9% | 245 | 99.2% | 98.5% |
| `src/mask.rs` | 282 | 281 | 99.6% | 95.7% | 100 | 100.0% | 96.8% |
| `src/tape.rs` | 646 | 644 | 99.7% | 100.0% | 240 | 100.0% | 82.3% |
| `src/fps.rs` | 218 | 218 | 100.0% | 100.0% | 73 | 100.0% | 98.7% |
| `src/jobs.rs` | 106 | 106 | 100.0% | 100.0% | 103 | 100.0% | 94.6% |
| `src/jobs/host.rs` | 80 | 80 | 100.0% | 100.0% | 38 | 100.0% | — |

## Baseline

- v1.4.6 (2026-10-21): unit tests 59.3% of lines (5,452 of 9,192), whole suite (126 tests) 83.6% (7,688 of 9,192). Its per-file figures are the last column above; `—` marks a file that didn't exist yet.
- 7a61aa2 (the job records' tests): unit tests 61.7% of lines, whole suite 84.3%.

## Critical paths not yet covered

- `src/home.rs` (0% by unit tests, 78% of its code by the whole suite): the neutral home's refusals (a home path that exists on this machine, no user namespaces, no `unshare`, a namespace the kernel refuses). A gap here can leak a real path into a frame.
- `src/config.rs` (73% of its code): the default config path from `XDG_CONFIG_HOME` or `HOME`, `~` expansion, and doctor's line for a config that is absent or has no HOME. The tests always name `PREC_CONFIG`, and unit tests can't set the environment without `unsafe`.
- `src/paths.rs` (87%): the CPU pinning a `[confine]` config asks for (`confine_here`, `cpu_set`, `build_cpus`), stopping a run's children, and a lock that has to be retried.
- `src/cache.rs` (85%): `prec cache`, `prec cache --json` and `prec cache prune` through the binary.
- `src/render.rs` (95%): a still's `--web` copies, a master whose frames, size or rate don't match what prec encoded, finished files moved to an `--out` on another filesystem, and clearing an old run folder another live run still holds.
- `src/main.rs` (93%): `prec archive --retry` and `--reindex` and `prec cache` dispatch, and the signal handler's SIGHUP branch.
- `src/cdp.rs` (93%): Chromium found without `XDG_DATA_HOME`, a Chromium that starts without a DevTools pipe, and a page that never finishes loading.

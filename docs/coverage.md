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

## Now (2026-10-22, after wave 12: the job records, archive and cache, paths, home, config and private-data scan, command line, recording and rendering tests)

Unit tests (326): **82.5% of lines** (13,948 of 16,903), 77.0% of functions, 81.9% of regions; **71.0% of code lines** (5,172 of 7,286).

| File | Lines | Covered | Lines % | Functions % | Code lines | Code % | Lines % at v1.4.6 |
|---|---:|---:|---:|---:|---:|---:|---:|
| `src/doctor.rs` | 166 | 68 | 41.0% | 72.7% | 115 | 19.1% | 0.0% |
| `src/main.rs` | 1,124 | 593 | 52.8% | 46.3% | 376 | 23.9% | 5.3% |
| `src/render.rs` | 1,668 | 932 | 55.9% | 52.0% | 1,023 | 34.3% | 32.0% |
| `src/replay.rs` | 418 | 263 | 62.9% | 69.8% | 268 | 45.9% | 8.8% |
| `src/config.rs` | 313 | 234 | 74.8% | 63.9% | 86 | 69.8% | 47.5% |
| `src/jobs.rs` | 106 | 81 | 76.4% | 72.7% | 103 | 76.7% | 17.9% |
| `src/cdp.rs` | 539 | 412 | 76.4% | 74.2% | 301 | 61.8% | 36.6% |
| `src/record.rs` | 1,086 | 835 | 76.9% | 78.9% | 586 | 60.1% | 57.6% |
| `src/jobs/env.rs` | 35 | 29 | 82.9% | 66.7% | 32 | 90.6% | — |
| `src/paths.rs` | 1,496 | 1,255 | 83.9% | 77.2% | 541 | 83.9% | 64.0% |
| `src/app.rs` | 890 | 752 | 84.5% | 78.0% | 446 | 72.6% | 41.4% |
| `src/jobs/util.rs` | 77 | 66 | 85.7% | 85.7% | 75 | 86.7% | — |
| `src/cache.rs` | 1,674 | 1,441 | 86.1% | 78.8% | 538 | 70.6% | 83.8% |
| `src/jobs/fit.rs` | 109 | 94 | 86.2% | 100.0% | 107 | 86.0% | — |
| `src/font.rs` | 152 | 138 | 90.8% | 66.7% | 39 | 71.8% | 0.0% |
| `src/private.rs` | 883 | 817 | 92.5% | 86.0% | 244 | 87.7% | 79.7% |
| `src/archive.rs` | 2,783 | 2,610 | 93.8% | 76.1% | 785 | 92.1% | 91.9% |
| `src/encode.rs` | 633 | 607 | 95.9% | 77.8% | 300 | 99.0% | 76.7% |
| `src/theme.rs` | 428 | 415 | 97.0% | 89.8% | 243 | 96.7% | 31.3% |
| `src/jobs/job.rs` | 234 | 228 | 97.4% | 96.2% | 230 | 97.8% | — |
| `src/jobs/rate.rs` | 47 | 46 | 97.9% | 100.0% | 45 | 97.8% | — |
| `src/home.rs` | 305 | 301 | 98.7% | 100.0% | 107 | 98.1% | 0.0% |
| `src/compose.rs` | 511 | 508 | 99.4% | 97.9% | 245 | 99.2% | 64.3% |
| `src/mask.rs` | 282 | 281 | 99.6% | 95.7% | 100 | 100.0% | 96.8% |
| `src/tape.rs` | 646 | 644 | 99.7% | 100.0% | 240 | 100.0% | 80.9% |
| `src/fps.rs` | 218 | 218 | 100.0% | 100.0% | 73 | 100.0% | 98.7% |
| `src/jobs/host.rs` | 80 | 80 | 100.0% | 100.0% | 38 | 100.0% | — |

Whole suite (457 tests, unit and integration): **96.3% of lines** (16,283 of 16,903), 89.9% of functions, 95.4% of regions; **96.6% of code lines** (7,037 of 7,286).

| File | Lines | Covered | Lines % | Functions % | Code lines | Code % | Lines % at v1.4.6 |
|---|---:|---:|---:|---:|---:|---:|---:|
| `src/jobs/fit.rs` | 109 | 94 | 86.2% | 100.0% | 107 | 86.0% | — |
| `src/config.rs` | 313 | 287 | 91.7% | 86.9% | 86 | 91.9% | 62.6% |
| `src/paths.rs` | 1,496 | 1,396 | 93.3% | 87.7% | 541 | 95.4% | 77.7% |
| `src/cache.rs` | 1,674 | 1,577 | 94.2% | 89.4% | 538 | 94.2% | 85.2% |
| `src/jobs/util.rs` | 77 | 73 | 94.8% | 100.0% | 75 | 97.3% | — |
| `src/cdp.rs` | 539 | 512 | 95.0% | 90.3% | 301 | 93.0% | 84.9% |
| `src/main.rs` | 1,124 | 1,070 | 95.2% | 88.1% | 376 | 97.1% | 80.9% |
| `src/render.rs` | 1,668 | 1,594 | 95.6% | 88.2% | 1,023 | 94.7% | 83.7% |
| `src/encode.rs` | 633 | 610 | 96.4% | 78.8% | 300 | 100.0% | 85.1% |
| `src/archive.rs` | 2,783 | 2,684 | 96.4% | 83.3% | 785 | 95.0% | 94.4% |
| `src/theme.rs` | 428 | 415 | 97.0% | 89.8% | 243 | 96.7% | 46.8% |
| `src/app.rs` | 890 | 863 | 97.0% | 93.9% | 446 | 98.4% | 69.5% |
| `src/replay.rs` | 418 | 406 | 97.1% | 86.0% | 268 | 97.8% | 86.1% |
| `src/jobs/env.rs` | 35 | 34 | 97.1% | 100.0% | 32 | 100.0% | — |
| `src/jobs/job.rs` | 234 | 228 | 97.4% | 96.2% | 230 | 97.8% | — |
| `src/jobs/rate.rs` | 47 | 46 | 97.9% | 100.0% | 45 | 97.8% | — |
| `src/private.rs` | 883 | 869 | 98.4% | 96.3% | 244 | 97.5% | 95.1% |
| `src/font.rs` | 152 | 150 | 98.7% | 86.7% | 39 | 100.0% | 87.5% |
| `src/home.rs` | 305 | 301 | 98.7% | 100.0% | 107 | 98.1% | 73.9% |
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

Figures are each file's code lines covered by the whole suite.

- `src/jobs/fit.rs` (86% at v1.4.6, before wave 15's tests, which cover every reachable branch; the implementer puts the measured figure here and moves the line out once it is): it fits each job record into the 4,096 bytes pci reads, dropping the stage list and then cutting the longest text fields; a wrong cut gives pci an oversized or unreadable record, or loses the fields it shows.
- `src/paths.rs` (95.4%): stopping a run's children when one ignores SIGTERM and has to be killed, and the CPU pinning and lowered priority a `[confine]` config applies in the child before it runs (that code runs after the fork, where no profile is written).
- `src/cache.rs` (94.2%): `prec cache prune` without `--json` (the kept-in-use line and the freed total), sizes shown in GB, the cache root from `HOME` when `XDG_CACHE_HOME` is unset, and entries skipped while moving the cache.
- `src/render.rs` (94.7%): a still's `--web` copies, a master whose frames, size or rate don't match what prec encoded, finished files moved to an `--out` on another filesystem, and clearing an old run folder another live run still holds.
- `src/archive.rs` (95.0%): judging whether a render with no `pinned` field is pinned (a tag, a rev that is the commit), a queued copy whose size doesn't match its manifest, and a catalog write that is short or interrupted.
- `src/cdp.rs` (93.0%): Chromium found without `XDG_DATA_HOME`, a Chromium that starts without a DevTools pipe, and a page that never finishes loading.
- `src/config.rs` (91.9%) and `src/home.rs` (98.1%): doctor's line for a config with no HOME, and the neutral home's refusal on a machine without user namespaces or with an `unshare` error other than a refusal.
- `src/main.rs` (97.1%): `prec clip` and `prec still` with `--themes` (the tests cover `prec render --themes`).

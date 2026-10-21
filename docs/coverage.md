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

Add `--json --output-path <file>` to the report for the per-file numbers, or `--html` to see the uncovered lines.

## Baseline (v1.4.6, 2026-10-21)

Unit tests: **59.3% of lines** (5,452 of 9,192), 51.9% of functions, 59.4% of regions.

| File | Lines | Covered | Lines % | Functions % |
|---|---:|---:|---:|---:|
| `src/doctor.rs` | 117 | 0 | 0.0% | 0.0% |
| `src/font.rs` | 40 | 0 | 0.0% | 0.0% |
| `src/home.rs` | 119 | 0 | 0.0% | 0.0% |
| `src/main.rs` | 341 | 18 | 5.3% | 4.8% |
| `src/replay.rs` | 294 | 26 | 8.8% | 12.1% |
| `src/jobs.rs` | 56 | 10 | 17.9% | 27.3% |
| `src/theme.rs` | 278 | 87 | 31.3% | 22.9% |
| `src/render.rs` | 1270 | 407 | 32.0% | 28.6% |
| `src/cdp.rs` | 372 | 136 | 36.6% | 34.1% |
| `src/app.rs` | 551 | 228 | 41.4% | 41.3% |
| `src/config.rs` | 99 | 47 | 47.5% | 36.0% |
| `src/record.rs` | 700 | 403 | 57.6% | 57.7% |
| `src/paths.rs` | 600 | 384 | 64.0% | 62.9% |
| `src/compose.rs` | 336 | 216 | 64.3% | 50.0% |
| `src/encode.rs` | 451 | 346 | 76.7% | 56.8% |
| `src/private.rs` | 305 | 243 | 79.7% | 65.9% |
| `src/tape.rs` | 277 | 224 | 80.9% | 64.0% |
| `src/cache.rs` | 1043 | 874 | 83.8% | 75.2% |
| `src/archive.rs` | 1637 | 1504 | 91.9% | 68.9% |
| `src/mask.rs` | 156 | 151 | 96.8% | 84.6% |
| `src/fps.rs` | 150 | 148 | 98.7% | 96.2% |

Whole suite (126 tests, unit and integration): **83.6% of lines** (7,688 of 9,192), 73.6% of functions, 83.0% of regions.

| File | Lines | Covered | Lines % | Functions % |
|---|---:|---:|---:|---:|
| `src/doctor.rs` | 117 | 0 | 0.0% | 0.0% |
| `src/theme.rs` | 278 | 130 | 46.8% | 28.6% |
| `src/config.rs` | 99 | 62 | 62.6% | 52.0% |
| `src/app.rs` | 551 | 383 | 69.5% | 73.0% |
| `src/home.rs` | 119 | 88 | 73.9% | 66.7% |
| `src/paths.rs` | 600 | 466 | 77.7% | 78.1% |
| `src/main.rs` | 341 | 276 | 80.9% | 81.0% |
| `src/tape.rs` | 277 | 228 | 82.3% | 68.0% |
| `src/render.rs` | 1270 | 1063 | 83.7% | 75.2% |
| `src/cdp.rs` | 372 | 316 | 84.9% | 78.0% |
| `src/encode.rs` | 451 | 384 | 85.1% | 60.5% |
| `src/cache.rs` | 1043 | 889 | 85.2% | 78.3% |
| `src/record.rs` | 700 | 597 | 85.3% | 80.8% |
| `src/replay.rs` | 294 | 253 | 86.1% | 72.7% |
| `src/font.rs` | 40 | 35 | 87.5% | 37.5% |
| `src/archive.rs` | 1637 | 1545 | 94.4% | 74.3% |
| `src/jobs.rs` | 56 | 53 | 94.6% | 100.0% |
| `src/private.rs` | 305 | 290 | 95.1% | 88.6% |
| `src/mask.rs` | 156 | 151 | 96.8% | 84.6% |
| `src/compose.rs` | 336 | 331 | 98.5% | 96.4% |
| `src/fps.rs` | 150 | 148 | 98.7% | 96.2% |

## Critical paths not yet covered

- `src/home.rs` (0% by unit tests, 74% whole suite): the neutral home and hostname a public recording shows; a gap here can leak a real path into a frame.
- `src/doctor.rs` (0%) and `src/theme.rs` (47%): the machine check's reports and exit codes, and the themes' colours.
- `src/config.rs` (63%), `src/paths.rs` (78%) and `src/app.rs` (70%): the machine config, the session environment, where child processes run, and the shots file.
- `src/main.rs` (5% by unit tests): the command line, its parse errors and exit codes, reached only through the integration tests.
- `src/replay.rs` (9% by unit tests) and `src/cdp.rs` (37%): frame timing and the headless browser link; the integration tests reach them, the unit tests barely do.
- `src/encode.rs` (61% of functions): the encoder's failure paths.

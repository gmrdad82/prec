---
name: prec-dev
description: "Develop prec itself, the deterministic terminal recorder and renderer in this repository: its Rust source, tests, bin/ scripts, docs and the skill it ships. Triggers: prec repo, bin/gate, bin/smoke, bin/install, bin/docs-bundle, the quick check, fast gate, src/replay.rs, record.rs, render.rs, encode.rs, paths.rs, tests/, skills/prec, Cargo.toml, CHANGELOG, a piece or branch for prec, a prec release. Excludes using prec to record, render or screenshot an app, or writing a render/terminal.toml or a .tape (that is prec's own user skill, skills/prec here, installed with `prec skill install`)."
---

# prec-dev

prec records an app's TUI or CLI in a pty to an asciicast `.cast`, replays it frame by frame through xterm.js in headless Chromium, and writes exact H.264 and PNG masters with a JSON manifest. One Rust binary, public, MIT, Linux is the target and macOS is built only. The same cast, flags and prec version give the same bytes: that is the product, so changes that move bytes are deliberate and logged. `AGENTS.md` (always loaded in the main checkout) holds the rules and its Speed section wins over any brief; this skill adds the how-to. Using prec to render an app is the `prec` skill (`skills/prec`), not this one.

## Layout

| Path | Holds |
| --- | --- |
| `src/main.rs`, `src/app.rs`, `src/tape.rs` | the clap command line and bare-`prec` summary; the `render/terminal.toml` recipe; the tape parser |
| `src/record.rs`, `src/replay.rs`, `src/cdp.rs` | pty recording and the cast; xterm.js replay in headless Chromium over its DevTools protocol |
| `src/compose.rs`, `src/picture.rs`, `src/font.rs`, `src/theme.rs` | window, ground, inset, closure and poster; PNG and colour; fonts; themes |
| `src/encode.rs`, `src/fps.rs`, `src/sound.rs`, `src/audio.rs` | H.264 master through pito-codecs; frame rates; AAC mux; the private PipeWire (Linux) |
| `src/render.rs`, `src/private.rs`, `src/mask.rs`, `src/home.rs` | the render pipeline and manifest; the private-data scan; masks; the neutral home namespace |
| `src/paths.rs`, `src/cache.rs`, `src/config.rs`, `src/doctor.rs` | confinement and cancel (`paths::confined`); the machine cache; config; `prec doctor` |
| `src/agent_skill.rs`, `src/skill.rs` | embeds and installs `skills/prec`; test-only checks of that skill against the command line |
| `tests/` | integration tests, one area per file, helpers in `tests/common/` |
| `bin/` | `gate`, `smoke`, `install`, `docs-bundle`, `marks` |
| `skills/prec/` | the user skill that ships inside prec (six files, embedded with `include_str!`) |
| `docs/` | `docs/site/` (guides bundled for the docs site), `docs/formats.md` (manifest, cast, cache reports), `docs/bench/`, `docs/coverage.md`, `docs/images/` |
| `examples/demo/` | the sample app every example and test uses; `render/terminal.toml` is prec's own recipe |
| `assets/`, `NOTICE` | vendored xterm.js and VHS themes with their licences; credits (every doc image must be named there) |

## How work goes

- A piece runs in its own git worktree on `dispatch/<id>-<slug>`, branched from `main`. Commit there: signed (`commit.gpgsign` is set), staged by name (never `git add -A`), message ending with the `Co-Authored-By:` trailer for the agent and model.
- Merge into `main` locally from the main checkout, which is the first line of `git worktree list`: `git merge --no-ff dispatch/<id>-<slug>` there. A piece never pushes, tags or releases, and never rewrites history.
- A worktree has only tracked files. `AGENTS.md`, `CLAUDE.md`, `HANDOVER.md`, `prompts/` and `dev/` are gitignored, so read `AGENTS.md` in the main checkout, and run the quick check there as `bin/gate --fast` itself.
- Work another repository needs is not done here: report it back for that repository's own session.
- A user-visible change adds a line to `CHANGELOG.md` under the next version's `## vX.Y.Z — unreleased` heading (add the heading when the top one is dated).

## Critical rules

- **Quick check while working, full suite only on request.** `bin/gate --fast` ends with `fast gate: green`; nothing merges without it. No integration tests, GPU or heavy runs, goldens or media unless the plan asks; name the files you did not run in your report.
- **prec writes only** under the caller's `--out` (default `tmp/renders`), `tmp/`, and the cache (`${XDG_CACHE_HOME:-~/.cache}/prec/`), never in an app's repo, and renders only the repository it runs in (`tests/source.rs` holds the refusal).
- **Chromium is headless only and starts through `paths::confined`.** prec opens no window and no compositor, and its sound never reaches the session's PipeWire.
- **Determinism is the contract.** Encoder threads stay fixed at 16, CPU-only, fps are exact rationals, and a manifest field is never renamed or repurposed (`docs/formats.md`). Anything that moves master bytes gets a CHANGELOG line.
- **Code, tests and assets name no other product:** use alpha, beta, site, demo. This repo is public, so no private paths, hosts, names or keys reach any tracked file.
- **Tests are deterministic and fast:** no sleeps, fixed seeds, each test owns its scratch folder (`common::scratch(group, name)`). A test that runs prec or Chromium goes in `tests/`, everything else in the module's `#[cfg(test)]` mod.
- **Cargo runs with `CARGO_NET_GIT_FETCH_WITH_CLI=true`** (pito-codecs is a git dependency pinned by tag) and `--locked`; `bin/gate --fast` sets it for you.
- **Check memory before heavy runs** (`free -m`), run anything that can crash under `prlimit --core=1:1 -- timeout <secs>`, and close any headless Chromium a check leaves (`pgrep -a chromium`).

## Command discovery

| Command | What it does |
| --- | --- |
| `bin/gate --fast` | the quick check: ShellCheck (if installed), `cargo fmt --check`, clippy with `-D warnings`, unit tests (nextest if installed, else `cargo test --bins`) |
| `bin/gate` | the same lint, then `cargo test --locked --bins`; no green line |
| `bin/gate --render` | `cargo test --locked --tests`: every integration test, through Chromium, needs a systemd user session; the full suite |
| `cargo test --locked --test <name>` | one integration file alone, e.g. `--test source` or `--test gate` (no Chromium) |
| `cargo build --release --locked --bin prec` | the binary at `target/release/prec`; `prec` on the PATH is the installed release, not your change |
| `target/release/prec doctor` | checks Chromium, fonts, git, config, cache, encoder |
| `PREC_AUDIO=off target/release/prec clip demo typing --app-dir examples/demo --size 720p --fps 30` | a draft render into `tmp/renders/`: one size, one theme, 30 fps |
| `bin/smoke target/release/prec` | records the demo at 640x360 with that binary; prints `smoke: <master>, <n> frames, <s> s` |
| `bin/docs-bundle [--bin PATH] [--out DIR]` | builds `docs-vX.Y.Z.tar.gz` from `docs/site/`, the binary's `--help`, `docs/formats.md` and the changelog; refuses a binary whose version differs from `Cargo.toml`, a page without frontmatter, an image missing from `docs/images/` or `NOTICE` |
| `bin/install [--uninstall]` | builds release, installs under `~/.local/opt/prec`, links `~/.local/bin/prec`, runs `prec skill install`; it changes the home folder, so only when asked |
| `bin/marks` | refreshes `docs/images/pitomd-wordmark.svg` from the brand checkout named by `PITO_ASSETS` |

Each tool's `--help` is the source of truth (`prec <command> --help`; bare `prec` prints every command). Config, cache and sound switches: `PREC_CONFIG`, `PREC_CACHE`, `PREC_CHROMIUM`, `PREC_DIRECT=1` (unconfined, for tests), `PREC_AUDIO=off`.

## Release

A release is cut by the maintainer's own script (not tracked) from a clean `main` whose `CHANGELOG.md` has the version's dated section and whose `Cargo.toml` version matches `prec --version`. It ships the Linux and macOS tarballs, `docs-vX.Y.Z.tar.gz` (`bin/docs-bundle`) and `SHA256SUMS`, and runs `public-check` and `bin/smoke` first. A piece does none of this unless its plan is a release.

## Known pitfalls

- `tests/gate.rs` pins `bin/gate`'s exact steps, ShellCheck file list included: change one, change the other, then run `cargo test --locked --test gate`.
- `skills/prec/*.md` are checked against the real command line by `src/skill.rs` in the unit tests: a new or renamed flag, command, `PREC_` variable, recipe key or tape step needs its skill text, and a new skill file needs an entry in `src/agent_skill.rs`'s file list. The README tables and `docs/site/` need the same edit; bare `prec` and `--help` text feed the docs bundle.
- The encoder version (`pito-codecs`) is in `Cargo.toml`, `README.md`, `docs/site/install.md` and `docs/site/guides/how-it-renders.md`: bump them together.
- `.public-allow` and `.gitleaksignore` list the few lines a public scan waives (the vendored xterm files, two neutral-home test lines): a new fixture that trips `public-check` is waived there with its reason, never disguised.
- A second recording of the same shot waits for the first (exclusive per tree); replays run at once.
- Integration tests leave their scratch under the tree's `tmp/` (gitignored); keep any new scratch there.

## Decision framework

1. **A flag, command, recipe key or tape step** → code and unit tests in its module, then `skills/prec`, README, `docs/site/`, CHANGELOG; `bin/gate --fast`.
2. **Anything that moves pixels or bytes** (`compose`, `replay`, `encode`, colour, fonts, themes) → the quick check cannot see it: say in the report that `--test determinism` and `--test frames` were not run unless the plan asked, and log the change.
3. **A bug in recording, Chromium or sound** → reproduce with the demo draft render above, fix, add a `tests/` file or case when it needs prec or Chromium, else a unit test.
4. **Docs or `docs/site/` only** → edit, then `bin/docs-bundle --bin target/release/prec` to check frontmatter, images and local paths.
5. **A dependency or its pin** → `Cargo.toml` and `Cargo.lock` together, credits in `NOTICE` for any ported code, the pitfall list above for the encoder.
6. **A release, a push or a tag** → not a piece's work: leave it for the maintainer's release flow.

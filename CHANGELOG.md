# Changelog

## v1.5.2 — 2026-10-23

### Changed

- Clips encode with pito-codecs 2.0.0 (was 1.16.3), in the same format as before: H.264 High, CRF 14, BT.709.

## v1.5.1 — 2026-10-23

### Added

- `Wait+Exit[@timeout]` waits until the session's shell exits and its terminal closes, and refuses like `Wait` on a timeout (15 s by default).

### Changed

- prec's own tests check every master by decoding it in-process with pito-codecs, the codec prec encodes with, so the test suite needs no ffmpeg or ffprobe. They also check each master's BT.709 colour tags.
- No prec test sleeps to wait for something: each waits on what the session prints or on its end.

## v1.5.0 — 2026-10-22

### Added

- A tape can resize the terminal mid-shot: `Resize <cols>x<rows>` resizes at once, and `Resize <cols>x<rows> 3s` eases there through every column and row (ease-in-out), so the app redraws at each width. The cast records each resize as an asciicast `r` event, and `prec render` replays it.
- In a clip that resizes, the text keeps one size, fitted to the largest grid, on a fixed canvas, and the window frame grows and shrinks with the grid from a fixed top-left corner. The manifest adds `max_grid` when a cast resizes.
- `bin/gate --fast` runs the fast checks (ShellCheck, rustfmt, clippy, the unit tests) and ends with `fast gate: green`.

### Changed

- After a resize, masks, `Wait`, `Idle`, `Squeeze` and the private-data scan all read the screen the render shows: a narrowed line is cut off, not re-wrapped, and a row shrink scrolls the top off as the terminal does.
- A `Squeeze` never cuts an eased resize short.

### Fixed

- A long repo name no longer makes a machine cache folder name too long for the filesystem, and a newline in a build command or bin folder can no longer give two builds the same cache key. Those builds rebuild once; `prec cache prune` clears the old entries.

## v1.4.8 — 2026-10-22

### Changed

- `prec cache --json prune --dry-run` is accepted with the flags in any order.
- An `--out` that reaches outside `tmp/` through a symlink, dangling or not, is now refused: prec judges a path by where it leads. A caller whose `tmp/renders` links to another disk gets a one-line refusal.
- `USER` and `LOGNAME` in a shot's `pass_env` are refused, since prec sets both itself.

### Fixed

- A `pass_env` value no longer shows in what a setup or teardown hook prints: hook output is masked line by line as it arrives.
- A clip refused by the private-data scan no longer leaves its run cast, which held the passed value, on disk.
- Redaction masks every passed value even when one contains or overlaps another.
- A bad `render/terminal.toml` is refused on one line naming its line and column, instead of several lines of the parser's snippet.
- `--bin` pointing at a folder without the app's binary, or a build that leaves a folder where the binary should be, is refused instead of recording "command not found".
- `prec doctor` and the Chromium lookup walk `PATH` themselves, so they work without bash.
- A bad `--size` or `--derive`, or a size past 16384 on a side, is refused before the app records, and opens no job record.
- Tapes with an `inf`, `nan` or overlong duration, unknown `Wait` modifiers, stray arguments, impossible Ctrl keys or zero or huge repeat counts are refused with their line instead of panicking or misbehaving; `Ctrl+@` works.
- A step typed after the session's terminal has closed is refused, instead of recording keys nobody received.
- An app that ignores SIGHUP and is still in the foreground when a tape ends is killed with its process group instead of hanging the recording.
- Casts with no events, a zero or oversized size, a negative time or times that go back are refused, naming the file and line.
- The browser timeout no longer reads "within 0 s" for a deadline under a second.
- A queued archive copy whose leaf or file name would land outside the archive is refused and settled; a queued web copy named with `@2x` archives; a queue entry whose record isn't an object no longer crashes every run; an `--older-than` too large to count is refused.
- A missing `cpus_from` file is told apart from one that lacks the key, and a home behind a dangling link is refused.

### Added

- Tests across every command's happy and bad paths: 469 tests, covering 96% of lines (docs/coverage.md has the numbers per file and how to measure them).

## v1.4.7 — 2026-10-22

### Changed

- prec writes its live job records itself, in exactly the same format as before, with one dependency fewer.
- A job record is never written through a symlink planted where its temporary file goes.
- The README has a line on where to ask questions (Discord).
- Test coverage is measured with cargo-llvm-cov; docs/coverage.md has the numbers per file and how to run it.

## v1.4.6 — 2026-10-19

### Changed

- `--fps` and a shot's `fps` take 23.98 too, for the film rate 24000/1001, as cameras and editors label it.
- Clip masters are encoded with pito-codecs 1.16.3.

## v1.4.5 — 2026-10-19

### Added

- Clips render at 24 and 48 fps and at the exact NTSC rates 24000/1001, 30000/1001, 48000/1001 and 60000/1001: `--fps` and a shot's `fps` take the rationals or 23.976, 29.97, 47.952 (or 47.95) and 59.94, every frame sits at its exact time, and the master MP4 carries the exact rate. Another decimal is refused with the rational to use.

### Changed

- Clip masters are encoded with pito-codecs 1.16.2.

## v1.4.4 — 2026-10-19

### Changed

- The check for private data before a render also catches Claude Code's usage limit lines, such as `You've used 77% of your weekly limit` or `session limit · resets 3pm`, unless a mask hides them.

## v1.4.3 — 2026-10-19

### Changed

- Clip masters are encoded with pito-codecs 1.16.0.
- Releases are built, tested and published on our own machines instead of GitHub Actions; the Linux and macOS downloads are the same as before.

## v1.4.2 — 2026-10-19

### Changed

- Clip masters are encoded with pito-codecs 1.15.1.

## v1.4.1 — 2026-10-18

### Changed

- Clip masters are encoded with pito-codecs 1.15.0.

## v1.4.0 — 2026-10-18

### Added

- `inset` in `terminal.toml`, or `--set inset=4%,15%,20%,4%`, places the terminal in a box inside the frame, so a vertical video keeps its text in the platform's safe zone. It takes one to four sides in percent or pixels, works for clips, stills and frames alike, and `prec render` keeps it from the cast.

## v1.3.0 — 2026-10-18

### Added

- A `Squeeze [gap] [/regex/]` tape step shortens every stretch where only a spinner or a counter moves (default 500 ms), keeping typed and printed text and `Sleep` at real speed; `Squeeze off` ends it, and the regex leaves status rows out.

### Fixed

- A tape that opens with `Hide` no longer shows the launch line in a first frame, and the private-data check no longer flags it.
- `Wait` regexes see a no-break space or any other Unicode space as a plain space, so `/⎿ +name/` matches what an app drew with a no-break space.

## v1.2.5 — 2026-10-17

### Changed

- prec encodes with pito-codecs 1.14.3.

## v1.2.4 — 2026-10-17

### Changed

- prec encodes with pito-codecs 1.14.2.

## v1.2.3 — 2026-10-16

### Changed

- prec encodes with pito-codecs 1.14.1.

## v1.2.2 — 2026-10-16

### Changed

- Clips are encoded with pito-codecs 1.14.0, the latest release of the encoder built into prec.

## v1.2.1 — 2026-10-16

### Changed

- Clips encode faster: the built-in encoder is now pito-codecs 1.13.0, and an 839-frame 720p30 clip encodes in about 2.4 s where 1.2.0 took about 4.2 s.
- The help, refusals and clip manifests name pcod (from pito-codecs) for making GIFs, WebP and web MP4s from a master, since pconv is gone.

## v1.2.0 — 2026-10-16

### Added

- `mask` in `terminal.toml`, for every shot or one: `{ match = "<regex>" }` or `{ rect = [col, row, cols, rows] }` hides that text or block of cells behind the theme's background in every frame, and every later render of the cast keeps it hidden.
- An `Idle 3s` tape step waits until the screen has stopped changing for that long, such as when an agent session finishes (`Idle@20m 5s` waits up to 20 minutes).
- The check for private data before a render also catches plan and organisation labels, such as `Claude Max` or `someone's Organization`, unless a mask hides them.

### Changed

- Clip masters are encoded inside prec by pito-codecs, so ffmpeg is no longer needed: the same quality within 0.2 dB at CRF 14, files about 3% smaller, and re-renders 1.6 to 3 times as fast (a 10-second cast at 1080p 60 fps in 16.9 s, was 27.4 s; at 720p 30 fps in 9.2 s, was 28.5 s).
- `[confine]` also runs the encode at nice 10, on its pinned CPUs.

## v1.1.0 — 2026-10-16

### Added

- `pass_env` in `terminal.toml` hands named variables, such as a token, from your environment to the recorded session without writing their values to the cast, the manifest or any message.
- `home` and `hostname` in `terminal.toml` record a session under a neutral absolute home and hostname in its own user and mount namespace, so its home, working folder and hostname are not yours (Linux only); `{home}` names that home in `cwd` and `launch`.
- Before replaying, prec refuses a picture that shows your home path, hostname, user name, an email address or a passed value, naming each and when it shows; `prec record` only warns, and `--allow-private` renders anyway.

## v1.0.3 — 2026-10-16

### Changed

- The README shows the PITO MD wordmark at its Licence section.
- prec builds on pito-jobs 1.0.2, the 1.0 line of the job records it writes; nothing changes in use.

## v1.0.2 — 2026-10-14

### Fixed

- The docs bundle's pages now load on the docs site: every title and description is a quoted string.

## v1.0.1 — 2026-10-14

### Added

- Each release carries the docs as docs-vX.Y.Z.tar.gz: an overview, install, fourteen guides with real commands and outputs, the full command line reference and the formats prec writes.

## v1.0.0 — 2026-10-14

### Changed

- prec is 1.0.0: the same code as 0.22.0, now its first stable release.
- The README shows every feature with a picture and measured numbers, frame counts against VHS v0.11.0 on the same tape included.
- The brand now reads PITO MD in the README, the notices and the docs.

## v0.22.0 — 2026-10-12

### Added

- An optional config.toml at ${XDG_CONFIG_HOME:-~/.config}/prec/config.toml (or PREC_CONFIG): the archive root, the default theme, Chromium's path and opt-in [confine].
- A default theme, Catppuccin Mocha, for an app whose terminal.toml has no [theme].
- PREC_CHROMIUM names the Chromium or Chrome binary.
- prec doctor shows the config it read.

### Changed

- The archive is off unless the config's archive or PITO_ARCHIVE names a root; a render then records the skip reason unconfigured.
- systemd-run confinement and CPU pinning run only when the config's [confine] asks for them.
- Omarchy themes are read where Omarchy installs them, at run time; prec themes import and themes/omarchy.json are gone.
- The machine cache moved to ${XDG_CACHE_HOME:-~/.cache}/prec/.
- Paths under the working folder print relative to it on a terminal; piped output keeps absolute paths.
- Input prec can't parse gets clap's short answer everywhere, a bare prec and archive with no flag included.
- pito-jobs comes from its public repository at v0.5.0.

### Removed

- Crash reporting: prec reports to no one.

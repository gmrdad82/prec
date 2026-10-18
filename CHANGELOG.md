# Changelog

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

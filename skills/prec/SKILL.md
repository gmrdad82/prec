---
name: prec
description: >
  Record an app's terminal UI or CLI with prec and render it as a clip (H.264 MP4 with a poster),
  a still (16-bit master PNG with 8-bit PNGs) or a saved cast again at another size, frame rate or
  theme. Use when asked to record, render, demo, screenshot or make a video of a terminal app, to
  write a render/terminal.toml recipe or a .tape, to pick a theme, or when prec refuses a picture
  for private data. GIFs and platform formats come from pcod. Triggers: prec, terminal recording,
  terminal clip, terminal screenshot, asciicast, .cast, .tape, VHS alternative,
  render/terminal.toml, prec doctor, mask, --allow-private.
---

# prec Skill

prec records an app's TUI or CLI in a pty to an asciicast `.cast`, replays it frame by frame
through xterm.js in headless Chromium, and writes exact masters plus a JSON manifest: a clip
(H.264 MP4, the app's sound on Linux, a poster PNG) or a still (a 16-bit sRGB PNG with 8-bit PNG
deliveries). The same cast, flags and prec version give the same bytes. prec runs inside the
app's own git repository and reads `render/terminal.toml` there.

## Topic Guides

Read the matching guide before starting:

- [`recipe.md`](recipe.md) - `render/terminal.toml`: build, environment, shots
- [`tapes.md`](tapes.md) - the keystrokes of one shot: keys, waits, hidden stretches
- [`privacy.md`](privacy.md) - pictures refused for private data, masks, `--allow-private`
- [`themes.md`](themes.md) - listing and choosing themes
- [`recast.md`](recast.md) - record once, render the cast again

## Commands

Bare `prec` prints one line per command; `prec <command> --help` has every flag. These run as
written from the top of prec's own repository, on its `examples/demo` app:

```sh
prec doctor
prec shots demo --app-dir examples/demo
PREC_AUDIO=off prec clip demo typing --app-dir examples/demo --size 720p --fps 30
prec still demo palette --app-dir examples/demo --size 1080p --derive 720p --web 800
prec record demo typing --app-dir examples/demo
prec render tmp/renders/demo/clip/demo-clip-typing.cast --size 720p --fps 30 --theme "Catppuccin Latte"
prec themes
```

- `prec doctor` checks the config, git, fontconfig, Chromium, xterm.js, themes, encoder, default
  font and a CJK fallback, plus optional libfaketime and sound; exit 1 when a required one is
  missing.
- `prec shots <app>` lists each shot: name, kind, status, line.
- `prec clip <app> <shot>` and `prec still <app> <shot>` record the shot, then render it; a shot
  of the other kind is refused.
- `prec record <app> <shot>` records only and prints the cast's path.
- `prec render <cast>` replays a cast without the app.
- `prec themes [--all]` lists themes.

## Flags

`clip`, `still` and `record` take the app's flags; `render` takes only the delivery ones.

- App: `--app-dir DIR` (the app's folder in this repository, default its top), `--bin DIR` (a
  prebuilt folder with the app's binary, skips the build), `--fixtures DIR` (a fixture set the
  app's recipe asks for).
- Size: `--size` is the largest delivery: `720p`, `1080p` (the default), `1440p`, `2160p`, `WxH` or
  a single `N` for a square; a clip needs even sides. `--derive` adds still sizes and `--web` web
  widths with `@2x` copies; a clip refuses both.
- Rate: `--fps` is 24, 30, 48, 60, 120 (the default) or 24000/1001, 30000/1001, 48000/1001,
  60000/1001. A clip with any other rate is made with pcod.
- Look: `--theme`, `--themes "A,B"` (one render per theme), `--font`, `--frame window|bare`,
  `--set key=value` (`grid=<cols>x<rows>`, `typing`, `settle`, `launch`, `closure`,
  `closure.crossfade`, `ground`, `inset`; `render` takes the last four).
- Clips only: `--poster first|before-closure|SECONDS`, `--audio-offset SECONDS` and
  `--audio-gain DB`; a still and `record` refuse them, `render` takes them for a clip's cast.
- Output: `--out DIR` (default `tmp/renders`, must lie inside the repository's `tmp/`), `--seed N`
  (default 7, recorded in the manifest), `--keep-frames`, `--no-encode` (frames only).
- `--allow-private` skips the private-data scan ([`privacy.md`](privacy.md)).
- `--youtube` and `--profile` are refused for clips: those formats belong to pcod.

## Rules

- `prec doctor` first on a machine you have not used prec on.
- Run prec from inside the app's repository. An `--app-dir` outside it, or a recipe whose `app` is
  not the one named, is refused.
- prec writes only under that repository's `tmp/` and the cache `~/.cache/prec` (`PREC_CACHE`
  moves it). Never write into the app's source tree and never empty the cache by hand during a
  render.
- Every render prints its manifest's path last. The manifest lists each file written with its
  sha256 (`master.mp4`, `poster.png`, `master.png`, the `.cast`); do not guess file names.
- A refusal is one line, `prec: ...`, exit 1. A flag clap rejects exits 2. A cancel by TERM, INT
  or HUP stops everything prec started, runs the recipe's `teardown` and exits 128 plus the signal.
- `PREC_AUDIO=off` turns the sound off, which most drafts do not need. `PREC_CONFIG` names another
  `config.toml` (keys `theme`, `chromium`, `[confine]`) and `PREC_CHROMIUM` another Chromium.
- A picture that shows a home path, host name, user name, email, a `pass_env` value, a plan or
  organisation label or a usage limit line is refused. Mask it, hide it or give the session a
  neutral `home`; `--allow-private` only for a render that stays private.
- Secrets go in `pass_env`, never in a tape or `[env]`.
- A second recording of the same shot in a working tree waits for the first.

## Decision Framework

1. A moving demo: a clip. One screen for docs: a still, with `--web` and `--derive`.
2. Several sizes, rates or themes of one session, or an app that is slow, needs data or must not
   run twice: `prec record` once, then `prec render` ([`recast.md`](recast.md)).
3. A GIF: render a 720p 30 fps clip, then convert it with pcod.
4. Checking work is a draft: one size, one theme, 30 fps, `PREC_AUDIO=off`. A master is the
   default 1080p and 120 fps unless the sizes, rates and themes asked for say otherwise. Draft
   first, master once.

Where the work is: no recipe yet, [`recipe.md`](recipe.md) then [`tapes.md`](tapes.md); a tape that
waits on the wrong text, [`tapes.md`](tapes.md); `the picture shows ...`,
[`privacy.md`](privacy.md); another look at a recording, [`themes.md`](themes.md) and
[`recast.md`](recast.md).

## Out of Scope

- GIFs, WebP and platform profiles: pcod, from the clip's master.
- Recording a graphical app, a browser or the desktop: prec records a pty only.
- Native Windows: use WSL. macOS (Apple Silicon) runs `doctor` and a draft clip, and its clips are
  silent; `home` and `hostname` are Linux only.

---
name: prec
description: >
  Set up and render terminal recordings of an app's TUI or CLI with prec: clips (H.264 MP4),
  stills (16-bit and 8-bit PNG), GIF sources and posters, from a recorded pty session replayed
  in headless Chromium. Use when asked to record, render, demo, screenshot or make a video or
  GIF of a terminal app, to write a render/terminal.toml recipe or a .tape, to pick a theme, to
  render a cast again at another size, frame rate or theme, or when prec refuses a picture for
  private data. Triggers: prec, terminal recording, terminal clip, terminal screenshot, asciicast,
  .cast, .tape, VHS alternative, render/terminal.toml, prec doctor, mask, --allow-private.
---

# prec Skill

[prec](https://github.com/gmrdad82/prec) records an app's TUI or CLI in a pty to an asciicast
`.cast`, replays it frame by frame through xterm.js in headless Chromium, and writes exact
masters: an H.264 clip, or a 16-bit still with 8-bit PNG deliveries, plus a JSON manifest.
Renders are deterministic: the same cast and flags give the same bytes.

## Topic Guides

Read the matching guide before starting:

- [`recipe.md`](recipe.md) - `render/terminal.toml`: setting up an app, its build, environment and shots
- [`tapes.md`](tapes.md) - writing a tape: keys, waits and hidden stretches
- [`privacy.md`](privacy.md) - pictures refused for private data, masks, `--allow-private`
- [`themes.md`](themes.md) - listing and choosing themes
- [`recast.md`](recast.md) - record once, then render the cast again

## Command Discovery

The CLI's help is the source of truth for flags; do not guess them.

```sh
prec --help
prec <command> --help
prec themes --all
prec shots <app>
```

Commands: `shots`, `clip`, `still`, `record`, `render`, `themes`, `doctor`.

## Critical Safety Rules

- Run `prec doctor` first on a machine you have not used prec on. It checks Chromium, fontconfig,
  git, the default font and a CJK fallback, and says what is missing.
- Set `PREC_AUDIO=off` when sound is not needed (most checks and drafts).
- prec runs inside the app's git repository and writes only under that repository's `tmp/`
  (outputs default to `tmp/renders`, `--out` moves them) and the cache `~/.cache/prec`
  (`PREC_CACHE` moves it). Never write into the app's source tree; do not clear the cache by hand
  while a render runs.
- The JSON manifest's path is printed last. Read the manifest for every file written and its
  sha256; do not guess file names.
- A picture that shows a home path, host name, user name, email, a value passed through
  `pass_env`, a plan or organisation label or a usage limit line is refused. Fix it with a
  mask, a neutral `home`, or the tape. Use `--allow-private` only for a render that stays private,
  never for one that is published.
- Put secrets in `pass_env`, never in a tape or `[env]`.
- Only one recording per working tree runs at a time; a second one waits.

## Decision Framework

**What to make**

1. A moving demo, or showing a flow over time: a clip (`prec clip <app> <shot>`).
2. One screen, for docs or a README: a still (`prec still <app> <shot>`), with `--web` widths
   for page copies and `--derive` for more sizes.
3. Several sizes, rates or themes of one session, or the app is slow, needs data or must not run
   twice: record once (`prec record`), then `prec render <cast>` as often as needed
   ([`recast.md`](recast.md)).
4. A GIF: render a 720p 30 fps clip master, then convert it with `pcod` or `ffmpeg`; prec itself
   writes the MP4 master.

**Draft or master**

- Checking work, trying a tape, tuning a mask: a draft. One size, one theme, 30 fps, into `tmp/`:
  `PREC_AUDIO=off prec clip <app> <shot> --size 720p --fps 30`.
- Delivering: a real master at the sizes, rates and themes asked for (defaults are 1080p and
  120 fps, so say what you want). Render drafts first, the master once.

**Where the work is**

- No `render/terminal.toml` yet: [`recipe.md`](recipe.md), then [`tapes.md`](tapes.md).
- A tape waits on the wrong text or is slow: [`tapes.md`](tapes.md).
- `the picture shows ...` refusal: [`privacy.md`](privacy.md).
- Another look at an existing recording: [`themes.md`](themes.md), [`recast.md`](recast.md).

## Example Requests

- "Make a 1080p clip of our TUI's tour" - recipe check, `prec doctor`, a 720p 30 fps draft, then
  the master.
- "Add a terminal shot for the settings screen" - a still shot in the recipe and a short tape.
- "Render that recording in a light theme at 4K" - `prec render <cast> --theme ... --size 2160p`.
- "prec says the picture shows the user name" - mask it, hide it, or give the shot a neutral `home`.
- "Record our agent session without my account showing" - `home`, `hostname`, `pass_env`, masks.
- "Which themes are there?" - `prec themes --all`.

## Out of Scope

- Converting or re-encoding clips into GIFs or platform profiles (`--youtube` and `--profile`
  belong to another tool, such as `pcod`).
- Recording a graphical app, a browser, or the user's own desktop: prec records a pty only.
- Editing prec's source; this skill is for using it.
- Native Windows: use WSL. macOS builds are untested for renders.

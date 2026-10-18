---
title: prec
description: Deterministic recordings of terminal apps into exact videos and stills, an alternative to VHS.
sidebar:
  order: 0
---

![A terminal: cat render/terminal/typing.tape shows a short tape; then prec clip demo typing --size 720p --fps 30 records the demo in a 60x16 pty, replays 114 frames at 1280x720 and prints its manifest, and ls lists the cast, the manifest, the master MP4 and the poster PNG.](images/record.gif)

prec records terminal apps into exact videos and stills, part of [PITO MD](https://pitomd.com). It is an alternative to [VHS](https://github.com/charmbracelet/vhs): you write a tape of keystrokes, and prec runs your app in a real pty, records the session once, then replays it frame by frame in headless Chromium at exactly 30, 60 or 120 fps, at any size up to 4K. The same recording always replays to the same bytes.

| | |
|---|---|
| Frames in a 10-second clip at 120 fps | **1,196**, every one (VHS v0.11.0 on the same tape: 214) |
| Re-render a 10-second cast at 1080p 60 fps | **16.9 s**, without running the app again |
| Two renders of one cast | **byte-identical** masters, same sha256 |
| Sizes and rates | any size up to **3840x2160**, at **30, 60 or 120 fps** |
| Binary | **5.7 MB**, stripped, fat LTO, its H.264 encoder built in (2.4 MB as the release `.tar.gz`) |
| Network | none: prec downloads nothing |

Measured on 2026-10-16 on an AMD Ryzen 9 9950X (prec and VHS pinned to the same 24 threads), with prec 1.2.0 and VHS v0.11.0; [How prec renders](guides/how-it-renders.md) gives every number and how it was taken.

## Where to start

- [Install](install.md) prec and its prerequisites, and record the sample app in 30 seconds.
- [Record your first clip](guides/first-clip.md) and read what prec wrote.
- [Set up your app](guides/set-up-your-app.md) with a `render/terminal.toml`, then [write a tape](guides/write-a-tape.md).
- [Take a still](guides/take-a-still.md), [render a cast again](guides/render-a-cast-again.md) at another size, rate or theme, and [choose a theme](guides/choose-a-theme.md).
- The [command line](reference/cli.md) and the [formats](reference/formats.md) prec writes for other tools.

## What it does

- **Every frame, at the rate you ask for.** prec records the app's output bytes with their timestamps, then steps a virtual clock at exactly 1/fps, so a clip has the shown time times the fps in frames.
- **Record once, render many.** The session is an asciicast v2 cast; `prec render` replays it at another size, rate or theme without running the app again.
- **Native pixels at every size.** The grid is fixed and the font is fitted to each size, so 4K shows exactly what 720p shows, drawn at its own pixels.
- **348 themes.** The app's own palette, VHS's built-in schemes, curated light and dark pairs, and installed Omarchy themes; `--themes` renders one recording in several at once.
- **Masters, not deliveries.** A clip is a silent H.264 master, a poster PNG and a JSON manifest; a still is a 16-bit PNG master with 8-bit deliveries.
- **Your app, your repository.** Each app keeps its own `render/terminal.toml` and tapes; prec builds it, runs its fixtures, and renders the working tree or any tag or commit.

prec runs on Linux and on Windows through WSL; its macOS binary is built on an Apple Silicon Mac, and renders on a Mac are untested. It is [MIT licensed](https://github.com/gmrdad82/prec/blob/main/LICENSE); the prec and PITO MD names and logos are not covered by that licence.

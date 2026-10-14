---
title: Render a cast again
description: Replay a recorded cast at another size, frame rate or theme without running the app.
sidebar:
  order: 5
---

Every render keeps its recording as `<stem>.cast`, an asciicast v2 file. `prec render` replays a cast at any size, rate or theme without building or running the app again, and the same cast with the same flags always gives the same bytes.

## Record only

`prec record` runs the app and keeps the cast, with no replay:

```sh
prec record demo typing --app-dir examples/demo
```

```text
recording demo typing in a 60x16 pty…
recorded 3.80s shown, 244 events → tmp/renders/demo/clip/demo-clip-typing.cast
```

## Replay

```sh
prec render tmp/renders/demo/clip/demo-clip-typing.cast --size 1080p --fps 60 --themes "Catppuccin Latte,TokyoNight"
```

```text
replaying demo typing: 228 frames at 1920x1080@60…
archive: skipped (unconfigured)
tmp/renders/demo/clip/demo-clip-typing-catppuccin-latte-1920x1080-60fps.json
replaying demo typing: 228 frames at 1920x1080@60…
archive: skipped (unconfigured)
tmp/renders/demo/clip/demo-clip-typing-tokyonight-1920x1080-60fps.json
```

The grid, the session, the shot's closure and its font come from the cast; `render` takes the delivery flags: `--size`, `--derive`, `--fps`, `--poster`, `--web`, `--theme`, `--themes`, `--font`, `--frame`, `--set closure…` or `--set ground=…`, `--out`, `--keep-frames`, `--no-encode` and `--no-archive`. A cast recorded for a still renders a still, and one recorded for a clip renders a clip.

## How long it takes

Re-rendering the 10-second cast of the sample app's `tour` shot on an AMD Ryzen 9 9950X (24 threads, prec 0.22.0):

![Bars, lower is better: seconds to replay and encode a 10-second cast with prec 0.22.0: 720p 30 fps 28.5 s, 1080p 60 fps 27.4 s, 2160p 30 fps 55.5 s, 2160p 120 fps 135 s.](../images/rerender.png)

Two 720p 30 fps renders of that cast wrote the same master, sha256 `25c4543804fb3357…`; the second took 15.4 s.

## Same bytes every time

A replay steps a virtual clock, not the wall clock, and the encoder's thread counts are fixed because x264 and zscale's output follows them. So a render's master doesn't depend on how busy the machine was or how many cores it has: keep the cast, and you can make the same master again later, or a 4K one, without the app or its data.

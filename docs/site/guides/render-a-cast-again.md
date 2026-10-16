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

## A picture with private data

Before it replays a cast, `prec clip`, `prec still` and `prec render` read the screen the cast shows, through every escape sequence, at every moment a frame can show: each 1/120 s of shown time, which every clip rate lands on, and a still's moment. A screen that shows your home path (`$HOME` and its real path), this machine's hostname or your user name (`$USER`, else `$LOGNAME`, three characters or more) as a whole word, an email address, or a value of four characters or more passed through `pass_env` refuses the render with exit 1, one line per finding at the first shown second it appears:

```text
prec: tmp/renders/demo/clip/demo-clip-typing.cast: the picture shows the user name at 2.41s; hide it in the tape or re-record
```

A passed value is never printed, only its variable's name. `prec render` has the names but no values in hand, so it skips them; `prec clip` and `prec still` check the values of the record they just made. What a hidden stretch prints and clears before the next `Show` is not in the picture and passes. `prec record` keeps the cast anyway and prints the same lines as warnings, so the take can be inspected. `--allow-private` on `clip`, `still` and `render` skips the scan, for a render that stays private, and the manifest records it as `"private_allowed": true`.

## How long it takes

Re-rendering the 10-second cast of the sample app's `tour` shot on an AMD Ryzen 9 9950X (24 threads, prec 0.22.0):

![Bars, lower is better: seconds to replay and encode a 10-second cast with prec 0.22.0: 720p 30 fps 28.5 s, 1080p 60 fps 27.4 s, 2160p 30 fps 55.5 s, 2160p 120 fps 135 s.](../images/rerender.png)

Two 720p 30 fps renders of that cast wrote the same master, sha256 `25c4543804fb3357…`; the second took 15.4 s.

## Same bytes every time

A replay steps a virtual clock, not the wall clock, and the encoder's thread counts are fixed because x264 and zscale's output follows them. So a render's master doesn't depend on how busy the machine was or how many cores it has: keep the cast, and you can make the same master again later, or a 4K one, without the app or its data.

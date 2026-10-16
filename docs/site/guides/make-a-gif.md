---
title: Make a GIF
description: Turn a clip's master into a GIF, WebP or web MP4 for a README or a page.
sidebar:
  order: 9
---

prec writes masters: a clip is one silent H.264 file at CRF 14, larger and sharper than a page needs. Make the delivery from it with pcod from [pito-codecs](https://docs.pitomd.com/pito-codecs/) or ffmpeg.

## With pcod

```sh
prec clip demo typing --app-dir examples/demo --size 720p --fps 30
pcod tmp/renders/demo/clip/demo-clip-typing-1280x720-30fps.master.mp4 demo.gif --size 1280 --fps 15
```

That is how the clip below was made: recorded at 1280x720 and 30 fps, made a 15 fps GIF at the same width.

![In a window on a dark ground, demo tick draws a progress bar, then demo show prints a truecolor gradient, a box with a wide CJK character, an arrow and blocks, and the 16 ANSI colours, before the clip crossfades back to its first frame.](../images/typing.gif)

## Keep it small

- Record at the size you will show: `--size 720p` for a GIF 1280 pixels wide, rather than a 4K master scaled down.
- Halve the rate for a GIF: record at 30 fps and convert at 15.
- Shorten the tape before shrinking the width: a few seconds of the one thing to show.
- Give the shot a closure (`closure = { crossfade = 0.4 }` or `"palindrome"`, [Frame, closure and poster](frame-closure-poster.md)) so the loop doesn't jump.
- Use `--frame bare` when the page already frames the clip.

## Stills

A still's 8-bit PNGs are already deliveries; `--web 800` adds an 800-pixel-wide copy and its `@2x` ([Take a still](take-a-still.md)).

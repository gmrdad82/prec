---
title: Take a still
description: Render a 16-bit master PNG with 8-bit deliveries, extra sizes and web copies.
sidebar:
  order: 4
---

A still shot (`kind = "still"`) renders one moment, 150 ms after its tape's last step, as a 16-bit sRGB master PNG and 8-bit PNG deliveries. With the sample app:

```sh
prec still demo palette --app-dir examples/demo --size 1080p --derive 720p --web 800
```

```text
recording demo palette in a 60x16 pty…
recorded 1.30s shown, 237 events → tmp/prec/run/demo-still-palette-cast-234771/demo-still-palette.cast
replaying demo palette to 2.35s at 1920x1080…
archive: skipped (unconfigured)
tmp/renders/demo/still/demo-still-palette-1920x1080.json
```

```text
tmp/renders/demo/still/
  demo-still-palette-1920x1080.master.png       56,418 bytes   16-bit sRGB, lossless
  demo-still-palette-1920x1080.png              38,395 bytes   8-bit delivery at the master's size
  demo-still-palette-1280x720.png               50,332 bytes   8-bit, from --derive 720p
  demo-still-palette-1920x1080-web-800.png      31,202 bytes   800x450, from --web 800
  demo-still-palette-1920x1080-web-800@2x.png   65,620 bytes   1600x900, its @2x copy
  demo-still-palette-1920x1080.cast             15,287 bytes
  demo-still-palette-1920x1080.json              4,768 bytes
```

## Sizes

- `--size` is the master's size: `720p`, `1080p`, `1440p`, `2160p`, `WxH`, or a single `N` for a square. The default is 1080p, or the shot's own `size`.
- `--derive` adds more delivery sizes from the same master, each no larger than it: `--derive 720p` or `--derive 1280x720,640x360`.
- `--web` adds web copies at the given widths, each also at `@2x`, keeping the master's aspect.

The master is drawn at its own size, never scaled; every derived size and web copy is resampled from it with Lanczos3 in linear light, so edges and colours hold up when they shrink.

## Native pixels

The grid is fixed by the recipe and the font is fitted to the render size, so a 4K still shows exactly what a 720p still shows, each drawn at its own pixels. Top: a crop of a 1280x720 still, enlarged three times; bottom: the same crop of the 3840x2160 still from the same tape.

![Two crops of the same box: the upper one, from the 720p still and enlarged three times, shows blocky pixels on the CJK character, the text and the arrow; the lower one, from the 4K still at its own pixels, shows the same glyphs smooth.](../images/sizes.png)

For the sharpest small image, render that size with `--size` instead of deriving it.

## Colour

The master is a 16-bit sRGB PNG, and every delivery an 8-bit PNG with an sRGB chunk. The terminal draws 24-bit truecolor, painted by xterm.js in Chromium with its colour profile forced to sRGB, so the same cast gives the same pixels on any machine.

## Several themes at once

`--themes` renders one recording in each theme, with the theme's slug in each file name; see [Choose a theme](choose-a-theme.md).

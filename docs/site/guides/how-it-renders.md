---
title: How prec renders
description: Record, replay, encode: why every frame lands, why the bytes repeat, and the measured numbers against VHS.
sidebar:
  order: 14
---

## Record

The app runs in a pty of exactly the recipe's grid, driven by the tape at real speed. Its output bytes are kept with monotonic timestamps as an asciicast v2 cast, with the input events and `hide`, `show`, `screenshot` and `end` markers ([Formats](../reference/formats.md#2-the-cast)).

## Replay

A vendored xterm.js 5.5.0 (canvas renderer, Unicode 11 widths) runs in headless Chromium (`--disable-gpu`, sRGB), started through `env -i` with a scratch `HOME`, `TZ=UTC` and `C.UTF-8`, so your locale and fontconfig never reach it. prec steps a virtual clock at exactly 1/fps, writes every cast byte up to that moment, waits for xterm to paint, and screenshots the page at the target size. Hidden stretches are cut, and a frame whose moment brings no new bytes reuses the previous one, so a mostly still terminal replays faster than a busy one.

xterm.js draws truecolor, box drawing, wide CJK and Nerd Font glyphs; the font is fitted to the render size so the grid fills it at its own pixels.

## Encode

A clip is one silent H.264 High master at the render's size and fps, encoded inside prec by [pito-codecs](https://docs.pitomd.com/pito-codecs/) 1.15.0 with no ffmpeg: its `quality` preset, CRF 14, no B-frames, 8-bit 4:2:0, a keyframe every two seconds, BT.709 limited range tagged in the stream. Chromium's 8-bit frames go to YUV with the BT.709 limited-range matrix and left-sited chroma, and a crossfade is blended from the two frames before that. The encoder's 16 threads are fixed, because its output follows its thread count; that keeps a master byte-identical on any machine. prec reads the MP4 back and checks its frame count and size before the manifest claims them.

## Atomic

A render builds its files in its scratch folder, then moves them into the output folder and writes the manifest last, so a failed or cancelled re-render never leaves new files beside an old manifest.

## Every frame, against VHS

prec doesn't capture the screen while the app runs, so a clip has the shown time times the fps in frames, however heavy the frame or the rate. One 10-second tape recorded by both tools at 1280x720:

![Paired bars, higher is better: frames in the clip for one 10-second tape at 1280x720. Asked for 30 fps, prec 299 and VHS 239; at 60 fps, 598 and 228; at 120 fps, 1196 and 214. VHS wrote a 25 fps file of 9.6, 9.1 and 8.6 seconds.](../images/frames-vs-vhs.png)

| asked for | prec frames | prec clip | prec wall time | VHS frames | VHS file | VHS wall time |
|---|---|---|---|---|---|---|
| 30 fps | 299 | 30 fps, 9.97 s | 20.5 s | 239 | 25 fps, 9.56 s | 13.9 s |
| 60 fps | 598 | 60 fps, 9.97 s | 21.7 s | 228 | 25 fps, 9.12 s | 14.3 s |
| 120 fps | 1,196 | 120 fps, 9.97 s | 23.4 s | 214 | 25 fps, 8.56 s | 15.2 s |

VHS finishes sooner, because it keeps what it can capture in real time; prec records in real time, then replays and encodes every frame at CRF 14.

Measured on 2026-10-16 by `docs/bench/bench.sh` in prec's repository: an AMD Ryzen 9 9950X on Linux, both tools pinned to the same 24 threads at nice 10, prec 1.2.0 with `PREC_DIRECT=1` and VHS v0.11.0, recording `examples/demo`'s `tour.tape` (about 10 seconds of typing, progress bars and the palette). The charts are drawn by [pito-compare](https://docs.pitomd.com/pito-compare/).

## Size and build

The Linux x86_64 binary is 5,935,688 bytes (5.7 MB; 2,508,494 bytes packed as the release's `.tar.gz`), built with fat LTO, one codegen unit and stripped; a cold `cargo build --release --locked` took 70.4 s with 4 build jobs. prec has 18 direct dependencies and 95 crates in its Linux build; the xterm.js files, the VHS themes and pito-codecs' H.264 encoder are compiled in, so it runs no encoder of yours and reads nothing from the network.

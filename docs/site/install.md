---
title: Install
description: Install prec on Linux, WSL or macOS, its prerequisites, and a 30-second start.
sidebar:
  order: 1
---

prec runs on Linux (x86_64), on Windows through WSL, and on macOS (Apple Silicon). Each [release](https://github.com/gmrdad82/prec/releases/latest) carries a Linux and a macOS binary with a `SHA256SUMS` file. There is no native Windows build: install the Linux binary inside WSL. On macOS the binary is built and its tests pass on GitHub's runner; renders on a Mac are untested.

## The binary

**Linux and WSL:**

```sh
v=$(basename "$(curl -fsSLo /dev/null -w '%{url_effective}' https://github.com/gmrdad82/prec/releases/latest)")
mkdir -p ~/.local/bin && curl -fsSL "https://github.com/gmrdad82/prec/releases/download/$v/prec-$v-linux-x86_64.tar.gz" | tar -xz -C ~/.local/bin prec
```

**macOS (Apple Silicon):**

```sh
v=$(basename "$(curl -fsSLo /dev/null -w '%{url_effective}' https://github.com/gmrdad82/prec/releases/latest)")
mkdir -p ~/.local/bin && curl -fsSL "https://github.com/gmrdad82/prec/releases/download/$v/prec-$v-macos-arm64.tar.gz" | tar -xz -C ~/.local/bin prec
```

`~/.local/bin` needs to be on your `PATH`. To check a download, fetch `SHA256SUMS` from the same release and run `sha256sum -c --ignore-missing SHA256SUMS` beside the `.tar.gz`.

**From source**, with Rust 1.97 or newer (replace `vX.Y.Z` with a release tag):

```sh
cargo install --locked --git https://github.com/gmrdad82/prec --tag vX.Y.Z prec
```

`prec --version` prints `prec <x.y.z>`.

## Prerequisites

prec drives tools you install yourself:

- **Chromium** or Google Chrome, run headless (`chromium`, `chromium-browser`, `google-chrome-stable` or `google-chrome` on the `PATH`, or `chromium` in the [config](guides/configure-prec.md)).
- **ffmpeg and ffprobe** with the `libx264` encoder and the `zscale` filter (libzimg).
- **fontconfig** (`fc-match`, `fc-list`), the default font **JetBrainsMono Nerd Font** (or pass `--font`), and a CJK font for wide characters.
- **git** and **bash**.

On Debian or Ubuntu:

```sh
sudo apt install chromium ffmpeg fontconfig fonts-noto-cjk git
```

On Arch:

```sh
sudo pacman -S chromium ffmpeg fontconfig noto-fonts-cjk ttf-jetbrains-mono-nerd git
```

On macOS with Homebrew, then point `chromium` in the config at `/Applications/Chromium.app/Contents/MacOS/Chromium`:

```sh
brew install ffmpeg fontconfig git
brew install --cask chromium font-jetbrains-mono-nerd-font
```

The JetBrainsMono Nerd Font is also at [nerdfonts.com](https://www.nerdfonts.com/font-downloads).

## Check the machine

`prec doctor` checks all of it and says what is missing. Here it is on an Arch Linux machine with everything in place and no config file (the home folder is shortened to `~`):

```text
$ prec doctor
ok   config                       ~/.config/prec/config.toml (absent; the defaults apply)
ok   ffmpeg                       /usr/bin/ffmpeg
ok   ffprobe                      /usr/bin/ffprobe
ok   fc-match                     /usr/bin/fc-match
ok   git                          /usr/bin/git
ok   chromium                     /usr/bin/chromium: Chromium 152.0.7977.82 Arch Linux
ok   xterm.js                     xterm.js 5.5.0, addon-canvas 0.7.0, addon-unicode11 0.8.0, embedded
ok   themes                       348 vendored from VHS v0.11.0
ok   ffmpeg encoder libx264       present
ok   archive root                 none configured; renders are never archived
ok   ffmpeg filter format         present
ok   ffmpeg filter blend          present
ok   ffmpeg filter concat         present
ok   ffmpeg filter zscale         present
ok   ffmpeg filter setparams      present
ok   default font                 JetBrainsMono Nerd Font (/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Regular.ttf)
ok   CJK fallback                 WDXL Lubrifont TC,WDXL 滑油字 TC
opt  libfaketime (optional)       absent; apps pin their own clock through [env]
ok   cache                        ~/.cache/prec (270 B)
```

## Start in 30 seconds

This repository carries a sample app, `examples/demo`, with three shots. Clone it and record one:

```sh
git clone https://github.com/gmrdad82/prec && cd prec
prec clip demo typing --app-dir examples/demo --size 720p --fps 30
```

```text
recording demo typing in a 60x16 pty…
recorded 3.80s shown, 284 events → tmp/prec/run/demo-clip-typing-cast-338683/demo-clip-typing.cast
replaying demo typing: 114 frames at 1280x720@30…
archive: skipped (unconfigured)
tmp/renders/demo/clip/demo-clip-typing-1280x720-30fps.json
```

That writes `tmp/renders/demo/clip/demo-clip-typing-1280x720-30fps.master.mp4`, its poster PNG, the cast and the JSON manifest whose path it prints last. [Record your first clip](guides/first-clip.md) walks through each file, and [Make a GIF](guides/make-a-gif.md) turns the master into the clip below.

![The clip that run makes: in a window on a dark ground, demo tick draws a progress bar, then demo show prints a truecolor gradient, a box with a wide CJK character, an arrow and blocks, and the 16 ANSI colours, before the clip crossfades back to its first frame.](images/typing.gif)

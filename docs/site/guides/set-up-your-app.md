---
title: Set up your app
description: Give your app a render/terminal.toml with its build, environment and shots.
sidebar:
  order: 2
---

prec runs inside your app's git repository and reads `render/terminal.toml` at its top, or in the folder `--app-dir` names. The recipe says how to build the app, what its pty looks like, and which shots it has; each shot points at a [tape](write-a-tape.md) of keystrokes. Everything prec writes lands under that repository's `tmp/`.

## A recipe

```toml
app = "myapp"
bin = "myapp"
grid = [120, 34]
frame = "bare"
setup = "{bin}/myapp-fixture >/dev/null"
teardown = "myapp server --stop || true"

[build]
command = "cargo build --release --locked --bin myapp --bin myapp-fixture"
bin_dir = "{target}/release"

[env]
MYAPP_DATA = "{scratch}/data"
MYAPP_NOW = "2026-09-29T10:00:00Z"

[theme]
background = "#eff1f5"
foreground = "#4c4f69"

[shots.tour]
kind = "clip"
line = "The terminal workspace stepping through its views"
tape = "render/terminal/tour.tape"
launch = "myapp tui"
settle = "3s"
closure = "palindrome"
```

Then:

```sh
prec shots myapp
prec clip myapp tour --size 1080p --fps 60
```

## The top level

| key | meaning |
|---|---|
| `app` | the product's slug, the first argument of every command |
| `bin` | the app's binary name |
| `grid` | the pty's `[cols, rows]`; it never changes with the render size |
| `frame` | `window` or `bare` ([Frame, closure and poster](frame-closure-poster.md)) |
| `font` | the font family; default JetBrainsMono Nerd Font |
| `ground` | the colour around the window, `#rrggbb` |
| `prompt` | the shell prompt in the pty; default `$ ` |
| `typing` | the delay between typed characters; default `40ms` |
| `cwd` | the folder the shell starts in; default this run's scratch folder |
| `setup`, `teardown` | shell commands run before and after the recording; `teardown` runs even when the recording fails or prec is cancelled |
| `fixtures_env` | the variable that names `--fixtures` to the app; default `PITO_<APP>_FIXTURES` |
| `[build]` | `command` builds the app and `bin_dir` is where its binaries land; `bin_dir` alone uses prebuilt binaries in the tree |
| `[env]` | variables for the pty, as written |
| `[theme]` | the app's own palette ([Choose a theme](choose-a-theme.md)) |

An unknown key is refused, so a typo never passes silently.

## Shots

Each `[shots.<name>]` is one render:

| key | meaning |
|---|---|
| `kind` | `clip` or `still`; `prec clip` refuses a still shot and the other way round |
| `tape` | the tape, relative to the recipe's folder |
| `line` | one line for `prec shots` |
| `launch` | a command typed hidden before the recording shows |
| `settle` | how long to wait after `launch` before the recording shows; default `800ms` |
| `closure` | how a clip ends: `"palindrome"`, `{ crossfade = 0.5 }`, or none |
| `poster` | `"first"`, `"before-closure"` or seconds of shown time |
| `size`, `fps` | the defaults for `--size` and `--fps` (otherwise 1080p and 120 fps) |
| `grid` | this shot's own grid |
| `status` | a free word shown by `prec shots` |
| `fixtures` | the name of the fixture set this shot needs; prec refuses to record it without `--fixtures` naming that set ([Render a tag or commit](render-a-revision.md)) |

## Placeholders

`{tmp}` (the repository's `tmp/`), `{scratch}` (this run's scratch), `{app}` (the checkout), `{bin}` (the cached build of this commit), `{target}` (the repository's shared cargo target dir in the [machine cache](manage-the-cache.md)) and `{fixtures}` expand the same way in `setup`, `teardown`, `launch`, `cwd`, every `[env]` value and the `[build]` command. `bin_dir` takes `{tmp}`, `{app}` and `{target}`.

## The pty's environment

The pty starts from a clean environment, so the recording doesn't depend on your shell: `TZ=UTC`, `LANG=C.UTF-8`, `TERM=xterm-256color`, `COLORTERM=truecolor`, a scratch `HOME`, `USER`, `LOGNAME`, every `[env]` value as written, and `PATH=<bin>:<system PATH>`. Pin anything else your app reads, such as a clock or a data folder, through `[env]`, as `MYAPP_NOW` and `MYAPP_DATA` do above.

## A working example

prec's own repository has two: `examples/demo/render/terminal.toml`, a sample app with prebuilt scripts (`bin_dir = "{app}/render/terminal/bin"`, no build command), and the repository's own `render/terminal.toml`, which builds prec and records the clip at the top of the [overview](../index.md).

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
| `pass_env` | names of variables whose values come from prec's own environment, such as a token; see [A session with no private data](#a-session-with-no-private-data) |
| `home` | an absolute neutral home for the session, such as `/srv/demo`, in its own user and mount namespace; Linux only |
| `hostname` | the session's hostname when `home` is set; default `prec` |
| `mask` | regions hidden in every shot's picture; see [Mask a region](#mask-a-region) |
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
| `mask` | this shot's own masks, added to the top-level ones |
| `fixtures` | the name of the fixture set this shot needs; prec refuses to record it without `--fixtures` naming that set ([Render a tag or commit](render-a-revision.md)) |

## Placeholders

`{tmp}` (the repository's `tmp/`), `{scratch}` (this run's scratch), `{app}` (the checkout), `{bin}` (the cached build of this commit), `{target}` (the repository's shared cargo target dir in the [machine cache](manage-the-cache.md)) and `{fixtures}` expand the same way in `setup`, `teardown`, `launch`, `cwd`, every `[env]` value and the `[build]` command. `bin_dir` takes `{tmp}`, `{app}` and `{target}`. `{home}` expands in `cwd` and `launch` only: to the `home` key's path when it is set, else to the scratch `HOME`.

## The pty's environment

The pty starts from a clean environment, so the recording doesn't depend on your shell: `TZ=UTC`, `LANG=C.UTF-8`, `TERM=xterm-256color`, `COLORTERM=truecolor`, a scratch `HOME`, `USER`, `LOGNAME`, every `[env]` value as written, every `pass_env` variable, and `PATH=<bin>:<system PATH>`. Pin anything else your app reads, such as a clock or a data folder, through `[env]`, as `MYAPP_NOW` and `MYAPP_DATA` do above.

## A session with no private data

Three top-level keys keep a live session, such as a coding agent at work, free of your own details:

```toml
home = "/srv/demo"
hostname = "pitomd"
cwd = "{home}/work"
pass_env = ["MYAPP_TOKEN"]
```

- `pass_env` gives the pty and the `setup` and `teardown` hooks each named variable with its value from prec's own environment when the record starts. The value is never written: the cast and the manifest list the names only, and no log line or error message prints it. A name that is unset refuses the record with `prec: pass_env NAME is not set` before any hook runs; a name also in `[env]`, or one prec sets itself (`HOME`, `PATH`, `TERM`, `PS1`, `SHELL`, `TZ`, `LANG`, `LC_ALL`, `COLORTERM`), is a config error.
- `home` runs the pty's shell under `unshare` from util-linux, in a user, mount and UTS namespace of its own: `home`'s parent (`/home` here) is covered by a tmpfs holding a bind of each folder that was there, so every path of yours stays reachable, plus the neutral home, bound to the run's scratch `HOME`. Inside, `HOME` is `home`, `USER` and `LOGNAME` are its last part unless `[env]` sets them, the hostname is `hostname`, and a `cwd` under the scratch home starts under `home`. `home` is an absolute path of two or more parts that doesn't exist on the machine, its parent does, and `[env] HOME` can't be set with it. The `setup` and `teardown` hooks run outside the namespace, as without `home`, and prepare the scratch home the session sees. Before recording, prec makes the namespace once and refuses with one line naming why when it can't, such as `unshare` or `hostname` missing, or unprivileged user namespaces turned off (Ubuntu's `apparmor_restrict_unprivileged_userns`). On a build for another system, `home` refuses with `prec: home needs Linux user namespaces`. `id -un` still prints your own user name, since the session keeps your uid.
- `hostname` names the namespace's host; it needs `home`.

Placeholders other than `{home}` keep their real paths, since only hidden steps (`launch`, the hooks) type them.

Before any replay, prec reads the screen the cast shows and refuses a picture that shows your home path, your hostname or user name as a whole word, an email address, a value the record passed through `pass_env`, a plan or organisation label (such as `Claude Max` or `someone's Organization`), or a usage limit line (such as `You've used 77% of your weekly limit`): [Render a cast again](render-a-cast-again.md#a-picture-with-private-data).

## Mask a region

Some text can't be kept off the screen: a banner that names the plan, an account line, a usage limit warning. A `mask` hides it in the picture by filling its cells with the theme's background:

```toml
mask = [
  { match = "Claude (Max|Pro|Team)" },
  { match = "You've used \\d+% of your .*limit" },
  { rect = [0, 0, 40, 2] },
]

[shots.agent]
kind = "clip"
tape = "render/terminal/agent.tape"
mask = [{ match = "[\\w.]+'s Organization" }]
```

- `match` is a regex tried on every row of the screen as it is at each frame; each match is covered, wherever and whenever it shows, and the cover follows the text as it scrolls.
- `rect = [col, row, cols, rows]` covers a fixed block of cells, counted from 0 at the top left; it must lie inside the grid.
- Top-level masks apply to every shot and a shot's own are added to them. They are written into the cast, so `prec render` covers the same regions at any size, rate or theme.
- The scan for private data reads the screen with the masks applied, so a masked label passes.

## A working example

prec's own repository has two: `examples/demo/render/terminal.toml`, a sample app with prebuilt scripts (`bin_dir = "{app}/render/terminal/bin"`, no build command), and the repository's own `render/terminal.toml`, which builds prec and records the clip at the top of the [overview](../index.md).

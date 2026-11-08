# Recipe: render/terminal.toml

prec reads `render/terminal.toml` at the app repository's top, or in the folder `--app-dir`
names. It says how to build the app, what its pty looks like, and which shots it has. An unknown
key is refused. Everything prec writes lands under that repository's `tmp/`.

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

Then `prec shots myapp` lists the shots and `prec clip myapp tour --size 720p --fps 30` renders one.

## Top level keys

- `app` the slug, the first argument of every command; `bin` the binary name.
- `grid = [cols, rows]` the pty size; it never changes with the render size.
- `frame` is `window` or `bare`; `font`, `ground` (`#rrggbb`), `prompt` (default `$ `),
  `typing` (default `40ms`), `cwd`.
- `setup` and `teardown` are shell commands before and after recording; `teardown` runs even when
  the recording fails or is cancelled.
- `[build]`: `command` builds the app, `bin_dir` is where binaries land. `bin_dir` alone uses
  prebuilt binaries already in the tree.
- `[env]` variables for the pty. The pty starts clean (`TZ=UTC`, `LANG=C.UTF-8`,
  `TERM=xterm-256color`, scratch `HOME`), so pin anything the app reads, such as a clock or data
  folder, here.
- `pass_env`, `home`, `hostname`, `mask`: see [`privacy.md`](privacy.md).
- `[theme]` the app's own palette: `background`, `foreground`, `selection`, `cursor` and the 16
  colours.
- `fixtures_env` names the variable that carries `--fixtures DIR` to the app.

## Shots

Each `[shots.<name>]`:

- `kind` is `clip` or `still`; `prec clip` refuses a still shot and the other way round.
- `tape` the tape path, relative to the recipe's folder; `line` one line for `prec shots`.
- `launch` a command typed hidden before the recording shows; `settle` how long to wait after it
  (default `800ms`).
- `closure`: `"palindrome"`, `{ crossfade = 0.5 }` or none. `poster`: `"first"`,
  `"before-closure"` or seconds.
- `size` and `fps` defaults (otherwise 1080p and 120 fps); `grid`; `mask`; `fixtures`.

## Placeholders

`{tmp}`, `{scratch}`, `{app}`, `{bin}`, `{target}` and `{fixtures}` expand in `setup`, `teardown`,
`launch`, `cwd`, `[env]` values and the build command. `bin_dir` takes `{tmp}`, `{app}`, `{target}`.
`{home}` expands in `cwd` and `launch` only.

## Prebuilt binary

`prec clip myapp tour --bin target/release` skips the build and puts that folder first on the
pty's `PATH`.

## Setting up an app, in order

1. `prec doctor`.
2. Write the recipe with a `[build]` (or `bin_dir`), a pinned `[env]`, and one shot.
3. Write the tape ([`tapes.md`](tapes.md)).
4. Draft: `PREC_AUDIO=off prec still myapp <shot> --size 720p` or a 30 fps clip into `tmp/`.
5. Look at the poster or still. Fix privacy findings ([`privacy.md`](privacy.md)).
6. Render the master once.

The app's repository may carry no prec output in git: `tmp/` stays ignored.

# Recipe: render/terminal.toml

prec reads `render/terminal.toml` at the app repository's top, or in the folder `--app-dir`
names. It says how to build the app, what its pty looks like and which shots it has. An unknown
key is refused, so a typo never passes. Everything prec writes lands under that repository's
`tmp/`.

## A recipe

This is the shape of prec's own `examples/demo/render/terminal.toml`, an app of prebuilt scripts:

```toml
app = "demo"
bin = "demo"
grid = [60, 16]
frame = "window"
typing = "35ms"

[build]
bin_dir = "{app}/render/terminal/bin"

[env]
DEMO_DATA = "{scratch}/data"

[theme]
background = "#1e1e2e"
foreground = "#cdd6f4"

[shots.palette]
kind = "still"
line = "Truecolor gradient, box drawing and the CJK 三"
tape = "render/terminal/palette.tape"
status = "SHIPPED"

[shots.typing]
kind = "clip"
line = "A command typed, a progress bar, then the palette"
tape = "render/terminal/typing.tape"
closure = { crossfade = 0.4 }
```

An app that has to be built adds `command` to `[build]`, such as `command = "make"` with
`bin_dir = "{app}/bin"`.

`prec shots demo --app-dir examples/demo` lists the shots and
`prec clip demo typing --app-dir examples/demo --size 720p --fps 30` renders one.

## Top level keys

- `app` the slug, the first argument of every command; `bin` the binary name; `grid = [cols, rows]`
  the pty size, which never changes with the render size.
- `frame` is `window` or `bare`; `font` (default JetBrainsMono Nerd Font); `ground` (`#rrggbb`
  around the window); `inset` (a box inside the frame for the grid, such as a video's safe zone:
  one to four of top, right, bottom, left as `N%` or `Npx`, like `"4%,15%,20%,4%"`);
  `prompt` (default `$ `);
  `typing` (default `40ms`); `cwd` (default the run's scratch folder).
- `setup` and `teardown` are shell commands before and after recording; `teardown` runs even when
  the recording fails or is cancelled.
- `[build]`: `command` builds the app, `bin_dir` is where its binaries land. `bin_dir` alone uses
  binaries already in the tree.
- `[env]` variables for the pty. The pty starts clean (`TZ=UTC`, `LANG=C.UTF-8`,
  `TERM=xterm-256color`, `COLORTERM=truecolor`, a scratch `HOME`, `USER`, `LOGNAME`, and `PATH`
  with the app's binaries first), so pin anything else the app reads, such as a clock or a data
  folder, here.
- `pass_env`, `home`, `hostname`, `mask`: see [`privacy.md`](privacy.md).
- `[theme]` the app's own palette: `background`, `foreground`, `selection`, `cursor` and the 16
  colours.
- `fixtures_env` names the variable that carries `--fixtures DIR` to the app; the default is
  `PITO_<APP>_FIXTURES` with the app's slug in capitals.

## Shots

Each `[shots.<name>]`:

- `kind` is `clip` or `still`, and `tape` the tape's path relative to the recipe's folder.
- `line` is one line for `prec shots`; `status` is a free word it shows beside the kind.
- `launch` is a command typed hidden before the recording shows; `settle` how long to wait after
  it (default `800ms`).
- `closure`: `"palindrome"` or `{ crossfade = 0.5 }`, none ends where the tape ends. `poster`:
  `"first"` (default), `"before-closure"` or seconds of shown time.
- `size` and `fps` are the defaults for `--size` and `--fps`; `grid`; `mask`.
- `fixtures` names a fixture set; the shot refuses to record without `--fixtures`.

## Placeholders

`{tmp}`, `{scratch}`, `{app}`, `{bin}`, `{target}` and `{fixtures}` expand in `setup`, `teardown`,
`launch`, `cwd`, `[env]` values and the build command. `bin_dir` takes `{tmp}`, `{app}`, `{target}`.
`{home}` expands in `cwd` and `launch` only.

## Prebuilt binary

`--bin DIR` skips the build and puts that folder first on the pty's `PATH`:
`prec clip demo typing --app-dir examples/demo --size 720p --fps 30 --bin examples/demo/render/terminal/bin`.

## Setting up an app, in order

1. `prec doctor`.
2. Write the recipe with a `[build]` (or `bin_dir`), a pinned `[env]` and one shot.
3. Write the tape ([`tapes.md`](tapes.md)).
4. Draft: `PREC_AUDIO=off prec still <app> <shot> --size 720p` or a 30 fps clip.
5. Look at the poster or still. Fix privacy findings ([`privacy.md`](privacy.md)).
6. Render the master once.

prec's output stays out of git: `tmp/` is ignored.

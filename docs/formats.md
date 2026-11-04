# prec's outputs

This file specifies everything prec writes for others to read: the render manifest, the cast and the cache reports. It is the one place these are specified: a consumer keeps a pointer here and the fields it reads. prec shares the manifest's core keys with [pfx](https://github.com/gmrdad82/pfx)'s renders.

A field is never renamed or given a new meaning. A new field may appear at any time, so readers ignore keys they don't know.

## 1. The manifest

Every `clip`, `still` and `render` writes one manifest, `<stem>.json`, beside its outputs in `<out>/<product>/<kind>/` (default `tmp/renders/`). The stem is `<product>-<kind>-<shot>[-<theme label>]-<W>x<H>` for a still, with `-<fps>fps` added for a clip, where `<fps>` is `24`, `30`, `48`, `60`, `120`, `23.976`, `29.97`, `47.952` or `59.94`. Wherever a manifest or a cast's `x_terminal_renderer` carries a rate (`fps`), a whole rate is a number (`30`) and an NTSC rate is its exact rational as a string (`"24000/1001"`, `"30000/1001"`, `"48000/1001"`, `"60000/1001"`); the master's MP4 track carries the same rate (timescale 24000, every sample 1001 long, at 24000/1001). The theme label is the slug of a theme other than the app's own (`--theme <scheme>`, `omarchy:<name>`, or each of `--themes`), such as `catppuccin-latte` or `omarchy-flexoki-light`.

The core keys, shared with pfx:

| key | meaning |
|---|---|
| `made` | when the command started, local RFC 3339 time with its offset; every theme of one run shares it |
| `caller` | `PITO_INVOKER` cleaned to a slug (lowercase letters, digits and single dashes), or `manual` |
| `tool` | `{"name": "prec", "version": "<x.y.z>"}` |
| `product`, `kind` | the app's product slug, and `still` or `clip` |
| `version` | the app's tag, else the first 12 hex digits of its commit, with `-dirty` for a dirty tree; `local` when no commit is known |
| `source` | `{"path", "sha256", "commit"}`: the cast this render replayed, and the app's commit |
| `params` | the render's flags: `size` ([w, h]), `size_arg`, `derive` (the extra still sizes), `fps` ([fps] for a clip, `null` for a still), `seed`, `set`, `frame`, `inset` (the inset as given, `null` when none), `theme`, `font`, `youtube`, `encode` (`false` with `--no-encode`), `web`, `poster` |
| `device` | `"CPU"` |
| `colour` | `master`, `delivery` and `terminal`, as text |
| `outputs` | `delivery`, `master`, `poster` and `resampling`, as text |
| `seconds` | the render's wall time, to a tenth of a second |
| `files[]` | every file written, below |

`files[]` holds one entry per file, in the order written: `file` (the name, relative to the manifest), `sha256`, `bytes`, `what` (a line of text), and where it applies:

| `role` | file | extra keys |
|---|---|---|
| `master` | a clip's `<stem>.master.mp4`, or a still's 16-bit `<stem>.master.png` | `fps` and `size` for a clip |
| `poster` | a clip's `<stem>.poster.png` | `fps`, `size`, and `frame`, the master frame it shows |
| `web` | a still's `<stem>-web-<px>.png` and `@2x` copies | `size` and `web` (the width asked for) |
| `cast` | `<stem>.cast`, the recording it rendered from | |
| none | a still's 8-bit delivery PNGs, the master's size and each `--derive` size | `size` |

The terminal renderer's own keys:

| key | meaning |
|---|---|
| `app` | `rev`, `commit`, `bin_dir`, `bin_sha256` and `terminal_toml_sha256`, copied from the cast; casts and manifests written before 1.7.0 also carry `tag` and `pinned`, which new ones drop and which still read and render |
| `shot`, `shot_sha256`, `tape_sha256`, `cast_sha256` | the shot's name, and the hashes of its resolved recipe, its tape and the cast |
| `fixtures` | `null`, or `{"path", "door", "files": [{"file", "sha256"}]}`: the materialized fixture set, the variable that named it to the app, and every file's hash |
| `pass_env` | the names the cast's record passed from prec's environment (`pass_env` in `terminal.toml`), never their values; `[]` when none |
| `private_allowed` | `true` when `--allow-private` skipped the scan for private data in the picture, else `false` |
| `theme` | `name`, `label` (`null` for the app's own theme), and the rendered `background` and `ground` as `#rrggbb` |
| `font` | `family`, `file`, `size_px` and `scale` |
| `grid` | `cols`, `rows`, `cell` (one cell's size in pixels) and `canvas` ([w, h]): the cast's starting grid |
| `max_grid` | only when the cast resizes: `cols` and `rows`, the largest grid it holds (the header's and every resize's, side by side), and `canvas` ([w, h]), the terminal's size at that grid, which the font is fitted to |
| `xterm` | `version`, `renderer`, `unicode` and `browser`, as text |
| `frames` | for a still, `still_at` (seconds of shown time) and `exact`; for a clip, `master` (frames in the master, `null` with `--no-encode`), `seconds`, `exact`, `shown_seconds`, `rendered`, `captured`, `reused`, `late` and `dropped` (always 0), `closure` and `render_seconds` |
| `encodes[]` | one per encode: `file`, `role`, `device`, `fps`, `size` and `args`, the encoder and its settings as words (until 1.1.0, ffmpeg's arguments) |

## 2. The cast

`<stem>.cast` is an asciicast v2 file: a header line, then one event per line.

- **Header:** `{"version": 2, "width": <cols>, "height": <rows>, "env": {"TERM": "xterm-256color", "SHELL": "/bin/bash"}, "x_terminal_renderer": {…}}`.
- **Events:** `[<seconds since the start, to the microsecond>, <code>, <data>]`, where the code is `o` for output, `i` for input, `r` for a resize of the terminal to `"<cols>x<rows>"` (each side 1 to 1000; the header keeps the starting grid), and `m` for a marker: `hide`, `show`, `screenshot` or `end`.
- **`x_terminal_renderer`** holds what a later `prec render <cast>` needs: `tool`, `product`, `shot`, `kind`, `grid`, `app_theme`, `frame`, `ground`, `inset` (`null` when none; absent before 1.4.0), `font`, `closure`, `size`, `fps`, `poster`, `typing`, `settle`, `launch`, `prompt`, `shown_seconds`, `app` (as in the manifest), `shot_sha256`, `tape_sha256`, `fixtures`, `env`, the pty's environment as `[name, value]` pairs without `HOME` and without the `pass_env` variables, `pass_env`, the names of the variables passed from prec's environment (never their values), `home` and `hostname`, the session's neutral home and hostname, or `null` when `terminal.toml` sets no `home`, and `mask`, the masks of `terminal.toml` and the shot as `{"match": "<regex>"}` or `{"rect": [col, row, cols, rows]}`, which every replay of the cast covers (`[]` when none; absent before 1.2.0), and `audio`, present only when the app made sound (below).

**Audio (Linux).** `prec record` and `prec clip` follow the app's PipeWire playback streams while it runs: each `Stream/Output/Audio` node whose process, by `pipewire.sec.pid` on the node or its client, is in the pty session's process tree is captured with `pw-record`, the captures are mixed on the cast clock, and the mix is written beside the cast as `<stem>.wav`: 48 kHz, 2 channels, 16-bit PCM, ending no later than the cast's `end` marker. `x_terminal_renderer.audio` is `{"file": "<stem>.wav", "sha256": "<hex>", "start": <seconds>, "rate": 48000, "channels": 2}`, where `start` is the WAV's first sample in seconds on the cast clock, the same clock as the event times, before any hidden stretch or squeeze cut is removed. The key is absent, and an earlier `<stem>.wav` beside the cast is removed, when no stream carried anything but digital silence, and also when `pw-record` or `pw-dump` is not on `PATH`, no PipeWire socket is in prec's `XDG_RUNTIME_DIR`, or the platform is not Linux. The pty's own `XDG_RUNTIME_DIR` is a private folder of links to `pipewire-0` and `pulse/native`; it is not in `env`.

## 3. The archive catalog line (gone)

Removed in 1.6.0: prec no longer archives renders.

## 4. The rest

- **Job records** are written by prec itself, in the format pito-jobs v1.0.3 wrote (byte for byte: the same folder, file names, keys and their order); README's Job records section names their folder, variables, stages and progress units. Each `<pid>-<started ms>.json` is one compact JSON object, at most 4096 bytes, with its keys in this order: `v` (1), `id` (`<host>:<pid>:<started>`), `host`, `pid`, `pid_start` (the process's start time in clock ticks since boot, field 22 of `/proc/self/stat`, or 0 where there is no `/proc`), `tool` (`prec`), `app`, `label`, `detail`, `invoker`, `parent`, `state` (`running`, `done` or `failed`), `stage`, `stages` (left out while empty), `stage_note`, `progress` (`{"done", "total", "unit"}`), `rate` (units a second, `null` for a `seconds` stage and until two progress samples exist), `pgpu` (`{"pid", "as"}`), `device`, `out` (an absolute path), `started`, `updated`, `ended` (Unix milliseconds), `exit` and `error`; a key with no value is `null`.
- **`prec --version`** prints `prec <x.y.z>`.

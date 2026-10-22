# prec's outputs

This file specifies everything prec writes for others to read: the render manifest, the cast, the archive catalog line and the cache reports. It is the one place these are specified: a consumer keeps a pointer here and the fields it reads. prec shares the manifest's core keys, the archive layout and the catalog with [pfx](https://github.com/gmrdad82/pfx)'s renders.

A field is never renamed or given a new meaning. A new field may appear at any time, so readers ignore keys they don't know.

## 1. The manifest

Every `clip`, `still` and `render` writes one manifest, `<stem>.json`, beside its outputs in `<out>/<product>/<kind>/` (default `tmp/renders/`). An archived render also stores it in its leaf as `manifest.json`. The stem is `<product>-<kind>-<shot>[-<theme label>]-<W>x<H>` for a still, with `-<fps>fps` added for a clip, where `<fps>` is `24`, `30`, `48`, `60`, `120`, `23.976`, `29.97`, `47.952` or `59.94`. Wherever a manifest or a cast's `x_terminal_renderer` carries a rate (`fps`), a whole rate is a number (`30`) and an NTSC rate is its exact rational as a string (`"24000/1001"`, `"30000/1001"`, `"48000/1001"`, `"60000/1001"`); the master's MP4 track carries the same rate (timescale 24000, every sample 1001 long, at 24000/1001). The theme label is the slug of a theme other than the app's own (`--theme <scheme>`, `omarchy:<name>`, or each of `--themes`), such as `catppuccin-latte` or `omarchy-flexoki-light`.

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
| `archive` | the archive outcome, below |

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
| `app` | `rev`, `tag`, `pinned`, `commit`, `bin_dir`, `bin_sha256` and `terminal_toml_sha256`, copied from the cast |
| `shot`, `shot_sha256`, `tape_sha256`, `cast_sha256` | the shot's name, and the hashes of its resolved recipe, its tape and the cast |
| `fixtures` | `null`, or `{"path", "door", "files": [{"file", "sha256"}]}`: the materialized fixture set, the variable that named it to the app, and every file's hash |
| `pass_env` | the names the cast's record passed from prec's environment (`pass_env` in `terminal.toml`), never their values; `[]` when none |
| `private_allowed` | `true` when `--allow-private` skipped the scan for private data in the picture, else `false` |
| `theme` | `name`, `label` (`null` for the app's own theme), and the rendered `background` and `ground` as `#rrggbb` |
| `font` | `family`, `file`, `size_px` and `scale` |
| `grid` | `cols`, `rows`, `cell` (one cell's size in pixels) and `canvas` ([w, h]) |
| `xterm` | `version`, `renderer`, `unicode` and `browser`, as text |
| `frames` | for a still, `still_at` (seconds of shown time) and `exact`; for a clip, `master` (frames in the master, `null` with `--no-encode`), `seconds`, `exact`, `shown_seconds`, `rendered`, `captured`, `reused`, `late` and `dropped` (always 0), `closure` and `render_seconds` |
| `encodes[]` | one per encode: `file`, `role`, `device`, `fps`, `size` and `args`, the encoder and its settings as words (until 1.1.0, ffmpeg's arguments) |

`archive` says what happened to the render, and README's Archive section says when each applies:

| `state` | keys beside it |
|---|---|
| `skipped` | `reason`: `no-archive`, `unconfigured` (no archive root is set), `test`, `ride`, `sample`, `frames-only` or `unpinned`; and `leaf`, the folder it would have had, when its names are safe |
| `queued` | `leaf`, the planned folder: copied into the machine cache's `archive-queue/`; when the archive is back, prec moves it into the archive and rewrites this manifest with the outcome |
| `archived` | `leaf`, where it landed (the planned folder, with `-<n>` when another render of the same second holds it), and `catalog`, the line in section 3 |
| `refused` | `error`: a name that isn't a safe folder name, or a manifest that can't be archived, with `leaf` when it was refused after queueing |
| `conflict` | `leaf` and `error`: the leaf holds another file under this render's `made`; the local copy stays |

Every `leaf` is relative to the archive's `Masters/` folder (since 0.21.0; before, to the archive root).

## 2. The cast

`<stem>.cast` is an asciicast v2 file: a header line, then one event per line.

- **Header:** `{"version": 2, "width": <cols>, "height": <rows>, "env": {"TERM": "xterm-256color", "SHELL": "/bin/bash"}, "x_terminal_renderer": {…}}`.
- **Events:** `[<seconds since the start, to the microsecond>, <code>, <data>]`, where the code is `o` for output, `i` for input, and `m` for a marker: `hide`, `show`, `screenshot` or `end`.
- **`x_terminal_renderer`** holds what a later `prec render <cast>` needs: `tool`, `product`, `shot`, `kind`, `grid`, `app_theme`, `frame`, `ground`, `inset` (`null` when none; absent before 1.4.0), `font`, `closure`, `size`, `fps`, `poster`, `typing`, `settle`, `launch`, `prompt`, `shown_seconds`, `app` (as in the manifest), `shot_sha256`, `tape_sha256`, `fixtures`, `env`, the pty's environment as `[name, value]` pairs without `HOME` and without the `pass_env` variables, `pass_env`, the names of the variables passed from prec's environment (never their values), `home` and `hostname`, the session's neutral home and hostname, or `null` when `terminal.toml` sets no `home`, and `mask`, the masks of `terminal.toml` and the shot as `{"match": "<regex>"}` or `{"rect": [col, row, cols, rows]}`, which every replay of the cast covers (`[]` when none; absent before 1.2.0).

## 3. The archive catalog line

Each archived render appends one JSON line to `<archive root>/Masters/catalog.jsonl`, the catalog it shares with pfx. The append holds `$XDG_RUNTIME_DIR/pito-archive/catalog.lock`, and `prec archive --reindex` rebuilds the file from both tools' leaves, `Masters/prec/` and `Masters/pfx/`, taking each line from the `archive.catalog` its leaf's `manifest.json` stored (a leaf without one is left out) and reading no other file.

`{"v": 1, "tool": "prec", "product", "shot", "version", "caller", "made", "path", "files": [{"name", "bytes", "sha256"}]}`

`tool` is `prec`, as is the leaf's folder under `Masters/`, and both tools' `archive --reindex` read these lines. Lines and leaves archived before 0.21.0 said `pito-replay`, under `<archive root>/pito-replay/` with the catalog at the root; a leaf moved under `Masters/` may still store the old `tool` and `path` in its `manifest.json`, so a reindex names each line's `tool` after the folder its leaf lies in under `Masters/` and its `path` after the leaf's place there.

`path` is the leaf relative to the archive's `Masters/` folder. `files` lists every archived file, as `{name, bytes, sha256}`:

- a clip's master, poster and cast;
- a still's 16-bit master, its 8-bit PNG delivery, each `--derive` size and `--web` copy, and its cast.

Lines written before 0.20.0 list only the masters, poster and cast. A file's name says what it is, as the role table in section 1 shows:

| name | file |
|---|---|
| `<stem>.master.mp4`, `<stem>.master.png` | master |
| `<stem>.poster.png` | poster |
| `<stem>-web-<px>.png`, `<stem>-web-<px>@2x.png` | web copy |
| `<stem>.cast` | cast |
| any other `<stem>.png` | an 8-bit delivery, at the size its stem ends in |

The same object is stored as the manifest's `archive.catalog`.

## 4. The cache reports

`prec cache --json` prints one JSON object and nothing else on stdout, in a shape shared by every PITO MD tool's cache report. It never waits on a lock.

| field | meaning |
|---|---|
| `v`, `tool`, `tool_version` | `1`, `"prec"` and the running version |
| `root`, `root_set_by` | the absolute cache root, and `"PREC_CACHE"` when that variable chose it, else `null` |
| `bytes`, `at` | the whole root's size in bytes, and now in Unix milliseconds |
| `entries[]` | one per entry, sorted by `section` then `name`: `section` (`git`, `src`, `work`, `target` or `bin`), `name` (as `cache` lists it), `repo` (the repo name without its hash), `path`, `bytes`, `used` (last use, Unix ms) and `in_use` (a render holds its lock) |
| `archive_queue` | `entries` and `bytes` waiting in `archive-queue/` for the archive |
| `prune` | `after_days` (30) and `stale_bytes`, what `cache prune` would free now |

`prec cache prune --json [--dry-run]` takes `--older-than` and `--all` as the text form does, and prints `{"v": 1, "freed": <bytes>, "removed": [{"section": …, "name": …, "bytes": …}], "kept_in_use": <n>}`: the bytes removed, the entries removed in `cache --json`'s order, and how many stale entries stayed because a render holds them. With `--dry-run` nothing is removed and the object says what a real prune would remove.

## 5. The rest

- **Job records** are written by prec itself, in the format pito-jobs v1.0.3 wrote (byte for byte: the same folder, file names, keys and their order); README's Job records section names their folder, variables, stages and progress units. Each `<pid>-<started ms>.json` is one compact JSON object, at most 4096 bytes, with its keys in this order: `v` (1), `id` (`<host>:<pid>:<started>`), `host`, `pid`, `pid_start` (the process's start time in clock ticks since boot, field 22 of `/proc/self/stat`, or 0 where there is no `/proc`), `tool` (`prec`), `app`, `label`, `detail`, `invoker`, `parent`, `state` (`running`, `done` or `failed`), `stage`, `stages` (left out while empty), `stage_note`, `progress` (`{"done", "total", "unit"}`), `rate` (units a second, `null` for a `seconds` stage and until two progress samples exist), `pgpu` (`{"pid", "as"}`), `device`, `out` (an absolute path), `started`, `updated`, `ended` (Unix milliseconds), `exit` and `error`; a key with no value is `null`.
- **`prec --version`** prints `prec <x.y.z>`.

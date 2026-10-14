---
title: Configure prec
description: The optional config.toml, confining Chromium and ffmpeg, and the environment variables prec reads.
sidebar:
  order: 10
---

prec works with no configuration. An optional `config.toml` lives at `${XDG_CONFIG_HOME:-~/.config}/prec/config.toml`, or wherever `PREC_CONFIG` points; `prec doctor` shows which file it read. An unknown key is refused.

```toml
theme = "TokyoNight"
chromium = "/opt/chromium/chrome"
archive = "~/Renders"

[confine]
slice = "build.slice"
cpus = "4-15"
```

| key | default | meaning |
|---|---|---|
| `theme` | `Catppuccin Mocha` | the theme for an app whose `terminal.toml` has no `[theme]` |
| `chromium` | found on the `PATH` | the Chromium or Chrome binary (`PREC_CHROMIUM` overrides it) |
| `archive` | none | an archive root; without one nothing is archived (`PITO_ARCHIVE` overrides it); see [Archive renders](archive.md) |
| `[confine]` | absent | on Linux with systemd, run Chromium and ffmpeg in memory-capped `systemd-run --user` scopes at nice 10 |
| `[confine] slice` | `build.slice` | the systemd slice for those scopes |
| `[confine] cpus` | unpinned | pin them with `taskset -c` to this CPU list |
| `[confine] cpus_from`, `cpus_key` | | or read the CPU list from a shell env file's key (`cpus_key`, default `BUILD_CPUS`) |

## Keep renders off your other work

A 4K or 120 fps render keeps Chromium and x264 busy for a minute or more. On a machine you are working on, `[confine]` runs them at nice 10 in a capped systemd scope, and `cpus` pins them to cores you can spare:

```toml
[confine]
cpus = "8-15"
```

`prec doctor` then also checks `systemd-run`, `prlimit` and `taskset`. The output doesn't change: x264's and zscale's thread counts are fixed, so a confined render writes the same bytes as an unconfined one.

## Environment variables

| variable | effect |
|---|---|
| `PREC_CONFIG` | the config file to read |
| `PREC_CHROMIUM` | the Chromium or Chrome binary |
| `PREC_CACHE` | moves the [machine cache](manage-the-cache.md) |
| `PITO_ARCHIVE` | the archive root |
| `PREC_DIRECT=1` | runs everything unconfined and never archives, for tests |
| `PITO_JOBS=off` | turns off the live [job record](job-records.md) |
| `PITO_INVOKER` | the `caller` a manifest records, cleaned to a slug; default `manual` |
| `NO_COLOR` | plain error output from the command line |

## Errors

Input prec can't parse gets clap's short answer on stderr with exit 2, and `--help` prints the full help. A refusal after parsing is one line, `prec: …`, with exit 1.

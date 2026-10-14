---
title: Manage the machine cache
description: What prec keeps between runs, where it lives, and how to prune it.
sidebar:
  order: 11
---

Everything reusable lives in one cache, shared by every repository: `${XDG_CACHE_HOME:-~/.cache}/prec/`, or `PREC_CACHE`. Outputs and scratch stay in each repository's `tmp/`.

| folder | holds | keyed by |
|---|---|---|
| `src/<repo>/<commit>/` | that commit's source, checked out by `git read-tree` with a private index, read-only (`{app}`) | repo and commit |
| `work/<repo>/` | the build tree, moved between commits so only changed files change | repo |
| `target/<repo>/` | the repo's shared cargo target dir, so a new commit rebuilds incrementally | repo |
| `bin/<repo>/<commit>-<build>/` | the built binaries (`{bin}`) | repo, commit and the build command's hash |
| `locks/` | one lock per entry, so a prune never removes an entry being built or used | |
| `archive-queue/` | renders waiting for an archive that was offline | |

A `<repo>` key is the repository's name and a hash of its origin URL, so every checkout and worktree of one repository shares entries.

## See it

```sh
prec cache
```

```text
cache: tmp/cache (0 B)
       set by PREC_CACHE
  (empty)
```

With entries, `prec cache` lists each with its size and last use. `prec cache --json` prints one JSON object, specified in [Formats](../reference/formats.md#4-the-cache-reports); it never waits on a lock.

## Prune it

```sh
prec cache prune                    # entries unused for 30 days
prec cache prune --older-than 2w    # or 12h, 90m, 30d
prec cache prune --all              # every entry not in use
prec cache prune --json --dry-run   # what a prune would remove, removing nothing
```

```text
{"freed":0,"kept_in_use":0,"removed":[],"v":1}
```

An entry a render holds stays, and the JSON's `kept_in_use` counts them. Removing an entry costs only time: the next render of that commit checks it out and builds it again.

prec downloads nothing, so the cache only ever holds your own repositories' sources and builds.

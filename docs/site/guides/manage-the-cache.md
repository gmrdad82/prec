---
title: Manage the machine cache
description: What prec keeps between runs, where it lives, and how it prunes itself.
sidebar:
  order: 11
---

Everything reusable lives in one cache, shared by every repository: `${XDG_CACHE_HOME:-~/.cache}/prec/`, or `PREC_CACHE`. Outputs and scratch stay in each repository's `tmp/`.

| folder | holds | keyed by |
|---|---|---|
| `work/<repo>/` | the build tree, where a clean tree builds at its HEAD | repo |
| `target/<repo>/` | the repo's shared cargo target dir, so every build is incremental | repo |
| `locks/` | one lock per entry, so a prune never removes an entry being built or used | |

A `<repo>` key is the repository's name and a hash of its origin URL, so every checkout and worktree of one repository shares entries.

prec records only the working tree. A clean tree builds at its HEAD in the build tree and a dirty tree builds in place; both build into the shared target dir. Each run copies the built binaries into its own scratch and runs them from there, so another build in the same target dir never changes a binary a run is using.

## It prunes itself

When a record or render starts, at most once a day, prec removes the entries unused for 30 days. It does this quietly and a failed prune never fails the run. An entry a run holds is never removed, and removing one costs only time: the next build recreates it. The day's stamp is the `.pruned` file in the cache root.

Folders an older prec left (`src/`, `bin/` and `git/`) are pruned by the same rule, and are not read for anything else.

prec downloads nothing, so the cache only ever holds your own repositories' builds.

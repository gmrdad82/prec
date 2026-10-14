---
title: Archive renders
description: Copy pinned renders into an archive folder with a catalog, and what happens when it is offline.
sidebar:
  order: 12
---

An archive is optional and off unless the config's `archive` (or `PITO_ARCHIVE`) names a root, such as a folder on a NAS. Then a render of a pinned, committed revision (`--rev <tag or commit>`, [Render a tag or commit](render-a-revision.md)) is copied to:

```text
<root>/Masters/prec/<app>/<shot>/<YYYY-MM-DD>_<HHMMSS>_<version>_<caller>/
```

with every file it wrote and its `manifest.json`, and one line is appended to `<root>/Masters/catalog.jsonl`, the catalog it shares with [pfx](https://docs.pitomd.com/pfx/)'s renders.

```toml
archive = "~/Renders"
```

## What is never archived

A working-tree render, a `--bin` build, a dirty revision, `--no-encode`, `--no-archive` and the `demo` sample are never archived. The manifest's `archive` records the outcome and, for a skip, its reason: `unconfigured`, `unpinned`, `no-archive`, `frames-only`, `sample`, `ride` or `test` ([Formats](../reference/formats.md#1-the-manifest)).

## Safe copies

A leaf is built as `<leaf>.partial/`, each file hashed as it is copied and checked against the manifest, then renamed into place, so a leaf that exists is complete. A second render in the same second gets `-<n>` after its leaf.

## An offline archive

When the root is offline, the render still succeeds: it waits in the [machine cache](manage-the-cache.md)'s `archive-queue/` with the state `queued`, and goes with the next render or with:

```sh
prec archive --retry
```

The manifest is rewritten with the outcome once the copy lands.

## Rebuild the catalog

```sh
prec archive --reindex
```

rebuilds `catalog.jsonl` from the leaves under `Masters/prec/` and `Masters/pfx/`. The catalog line's fields are in [Formats](../reference/formats.md#3-the-archive-catalog-line).

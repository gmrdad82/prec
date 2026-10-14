---
title: Render a tag or commit
description: Render any revision of your app with --rev, skip the build with --bin, and pass fixtures.
sidebar:
  order: 8
---

By default prec renders the working tree as it is. `--rev` renders any tag or commit of the repository instead, and that is the render to keep: its manifest names a pinned revision, and only such a render is [archived](archive.md).

## A tag or commit

```sh
prec clip myapp tour --rev v1.4.0 --size 1080p --fps 60
```

prec reads that revision from git without touching your index or working tree: it checks the commit's source out into the [machine cache](manage-the-cache.md) with a private index, builds it with the recipe's `[build]` command in a build tree shared by every commit, and keeps the binaries per commit. A second render of the same commit reuses the build; a new commit rebuilds incrementally in the repository's shared cargo target dir.

The recipe and the tapes come from that revision too, so an old tag renders as it looked then. The manifest's `version` is the tag (or the commit's first 12 hex digits), and `app.pinned` is `true`.

A working-tree render is never pinned: its `version` ends in `-dirty` when the tree has changes, and a dirty tree builds incrementally without being stored as a commit.

## A prebuilt binary

`--bin DIR` skips the build and puts `DIR` first on the pty's `PATH`:

```sh
cargo build --release
prec clip myapp tour --bin target/release
```

A `--bin` render is never [archived](archive.md).

## Fixtures

An app that needs data to show can take a fixture folder: `--fixtures DIR` passes it to the app as `{fixtures}` in the recipe and in the variable `fixtures_env` names (default `PITO_<APP>_FIXTURES`). A shot with `fixtures = "<set>"` refuses to record without a folder of that name. The manifest records the folder, the variable and every file's sha256.

## One recording at a time

Recording one shot is exclusive in a working tree: a second `prec clip myapp tour` waits for the first. Replays of a cast at different sizes, rates or themes run at once.

## Cancel

TERM, INT and HUP stop everything prec started, process groups and all, run the recipe's `teardown`, and exit 128 plus the signal. A render builds its files in its scratch folder and writes the manifest last, so a cancelled re-render never leaves new files beside an old manifest.

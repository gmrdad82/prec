---
title: Job records
description: The live progress record every render publishes for a dashboard to show.
sidebar:
  order: 13
---

Every `clip`, `still`, `record` and `render` publishes a live job record, which prec writes itself, so a dashboard can show what is rendering and how far it got.

- The record lives in RAM only, in `$XDG_RUNTIME_DIR/pito-jobs/`, or `PITO_JOBS_DIR`; on macOS without `XDG_RUNTIME_DIR`, in `$TMPDIR/pito-jobs/`.
- It is one JSON file per render, `<pid>-<started ms>.json`, rewritten atomically, at most once a second for progress, and at most 4 KiB.
- Its stages are `build`, `record`, `replay`, `master` (a clip) or `derive` (a still), and `archive`, each with its progress.
- `PITO_JOB_ID` names its parent job, `PITO_INVOKER` its invoker and `GPU_QUEUE_PID` its GPU queue.
- With none of those folders there is no record, `PITO_JOBS=off` turns it off, and a record that can't be written never fails the render.

The folder and the `PITO_JOBS*` variables keep their names, so a dashboard that already reads these records keeps reading prec's.

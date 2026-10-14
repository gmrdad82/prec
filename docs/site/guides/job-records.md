---
title: Job records
description: The live progress record every render publishes for a dashboard to show.
sidebar:
  order: 13
---

Every `clip`, `still`, `record` and `render` publishes a live job record through [pito-jobs](https://docs.pitomd.com/pito-jobs/), so a dashboard can show what is rendering and how far it got.

- The record lives in RAM only, in `$XDG_RUNTIME_DIR/pito-jobs/`, or `PITO_JOBS_DIR`.
- Its stages are `build`, `record`, `replay`, `master` (a clip) or `derive` (a still), and `archive`, each with its progress.
- Without `XDG_RUNTIME_DIR` there is none, and `PITO_JOBS=off` turns it off.

The record's format belongs to pito-jobs; prec only fills in its stages.

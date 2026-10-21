# Inspiration

Code prec carries from other projects, with where it came from.

## pito-jobs

prec's job-record writer in `src/jobs/` is copied from pito-jobs
(https://github.com/gmrdad82/pito-jobs), MIT, by the same owner as prec, at
commit 2d0689da084a7186aa2794bd192a70c91af24cf4 (tag v1.0.3), so the records
it writes stay byte for byte those pito-jobs wrote.

- `src/jobs/job.rs` from `src/job.rs`: `start_with` and `Job` with `detail`,
  `device`, `out`, `stage`, `stages`, `stage_note`, `progress`, `finish` and
  `fail`, its throttle and its atomic commit; `pgpu`, `rate` and `child_env`
  are left out.
- `src/jobs/record.rs` from `src/record.rs`: `Record`, `Progress` and `Pgpu`
  with their serde attributes; `read_dir` and `Progress::fraction` are left
  out.
- `src/jobs/fit.rs` from `src/fit.rs`, whole: the 4 KiB cap and its order of
  shrinking.
- `src/jobs/rate.rs` from `src/rate.rs`, whole: the rate over the longer of
  5 s and the last two updates.
- `src/jobs/env.rs` from `src/env.rs`: `Settings` and the job folder;
  `data_dir` is left out.
- `src/jobs/host.rs` from `src/host.rs`: the host name and the process start
  time; the host name asks the OS through nix's `gethostname` instead of
  calling libc directly.
- `src/jobs/util.rs` from `src/util.rs`: `Clock`, `atomic_write`, `filled`
  and `process_id`; `read_capped` is left out.
- `src/jobs/tests.rs` from `src/tests.rs`: the tests of the parts above.

The heartbeats, the readers and the progress lines of `live.rs`, `op.rs` and
`read_dir` are not carried.

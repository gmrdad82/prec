# Privacy

Before any replay, `prec clip`, `prec still` and `prec render` read the screen the cast shows, at
every 1/120 s of shown time. A render is refused (exit 1) when the picture shows:

- the home path, or this machine's host name or user name as a whole word (three characters or
  more);
- an email address;
- a value (four characters or more) passed through `pass_env`;
- a plan or organisation label, or a usage limit line.

The refusal is one line per finding, at the first shown second it appears:

```text
prec: <cast>: the picture shows the user name at 0.52s; hide it in the tape, mask it in terminal.toml or re-record
```

The finding names what it saw (`the home path`, `the hostname`, `the user name`, `an email`,
`a passed variable NAME`, `a plan label`, `an organisation label`, `a usage limit`), never a value.
`prec record` keeps the cast anyway and prints the same findings as warnings. What a hidden stretch
prints and clears before `Show` is not in the picture and passes; so does what a mask covers.

## Fixes, in order of preference

1. Hide it in the tape (`Hide` ... `Show`, or the shot's `launch`) and clear the screen before
   `Show` ([`tapes.md`](tapes.md)).
2. Give the session a neutral identity (Linux only):

   ```toml
   home = "/srv/demo"
   hostname = "demo"
   cwd = "{home}/work"
   pass_env = ["DEMO_TOKEN"]
   ```

   `home` is an absolute path of two or more parts that does not exist while its parent does; the
   shell then runs in its own user, mount and UTS namespace with `HOME`, `USER` and the host name
   neutral, and `[env]` cannot set `HOME`. `hostname` needs `home` (default `prec`). `pass_env`
   hands each named variable's value from prec's own environment to the pty and the hooks; an
   unset name refuses the record (`prec: pass_env NAME is not set`) and the value is never
   written, only the name.
3. Mask the text in every frame:

   ```toml
   mask = [
     { match = "Plan: \\w+" },
     { match = "used \\d+% of your .*limit" },
     { rect = [0, 0, 40, 2] },
   ]
   ```

   `match` is a regex tried on every screen row at each frame, and the cover follows scrolling
   text; `rect = [col, row, cols, rows]` covers fixed cells, counted from 0, inside the grid. A
   shot's `mask` adds to the top-level one. Masks are written into the cast, so `prec render`
   applies them at any size, rate or theme, and the scan reads the screen with them applied.

## --allow-private

`--allow-private` on `clip`, `still` and `render` skips the scan, and the manifest records
`"private_allowed": true`. Use it only for a render that stays private, never to get a published
render past the check.

`prec render` has the `pass_env` names but not their values, so it skips those; `clip` and `still`
check the values of the record they just made.

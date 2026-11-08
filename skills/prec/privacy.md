# Privacy

Before any replay, `prec clip`, `prec still` and `prec render` read the screen the cast shows, at
every 1/120 s of shown time. A render is refused (exit 1) when the picture shows:

- the home path, or this machine's host name or user name as a whole word (three characters or more);
- an email address;
- a value (four characters or more) passed through `pass_env`;
- a plan or organisation label, or a usage limit line.

```text
prec: tmp/renders/demo/clip/demo-clip-typing.cast: the picture shows the user name at 2.41s; hide it in the tape, mask it in terminal.toml or re-record
```

`prec record` keeps the cast anyway and prints the same findings as warnings. What a hidden
stretch prints and clears before `Show` is not in the picture and passes; so does what a mask
covers.

## Fixes, in order of preference

1. Hide it in the tape (`Hide` ... `Show`, or `launch`) and clear the screen before `Show`.
2. Give the session a neutral identity (Linux only):

   ```toml
   home = "/srv/demo"
   hostname = "demo"
   cwd = "{home}/work"
   pass_env = ["MYAPP_TOKEN"]
   ```

   `home` is an absolute path of two or more parts that does not exist, whose parent does; the
   shell runs in a user, mount and UTS namespace, with `HOME`, `USER` and the host name neutral.
   `hostname` needs `home`. `pass_env` hands a variable from prec's own environment to the pty and
   hooks; its value is never written, only its name.
3. Mask the text in every frame:

   ```toml
   mask = [
     { match = "Claude (Max|Pro|Team)" },
     { match = "You've used \\d+% of your .*limit" },
     { rect = [0, 0, 40, 2] },
   ]
   ```

   `match` is a regex tried on every screen row at each frame and follows scrolling text; `rect =
   [col, row, cols, rows]` covers fixed cells inside the grid. A shot's `mask` adds to the top
   level ones. Masks are written into the cast, so `prec render` applies them at any size or theme.

## --allow-private

`--allow-private` on `clip`, `still` and `render` skips the scan; the manifest records
`"private_allowed": true`. Use it only for a render that stays private. Never use it to get a
render past the check that will be published.

`prec render` has no `pass_env` values, so it skips those; `clip` and `still` check them.

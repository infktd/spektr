# Demos

VHS tapes of spektr on the fictional fixture case. `cargo xtask demos` builds a release binary and renders every tape to `demos/out/<name>.gif`, with all network traffic replayed from `crates/spektr-modules/fixtures` (test mode, fixed clock). Needs [VHS](https://github.com/charmbracelet/vhs) (and its `ttyd` and `ffmpeg`).

| Tape | Shows |
|---|---|
| `tutor.tape` | `spektr tutor`: the coach strip and the first lessons |
| `pivot.tape` | seed a username, accounts streaming in, pivot, back, the timeline |
| `capture.tape` | the avatar comparison, `v` to confirm, `c` to capture a profile |
| `finding-export.tape` | `f` a finding, `e` edit, checks, `r` ready, `x` export, then `spektr export` |

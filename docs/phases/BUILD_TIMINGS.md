# Build timings

## Phase 0 baseline (2026-10-20)

Measured on **Jay's M1 Pro**, the primary dev machine, by the building agent running locally on it.

| | |
|---|---|
| Machine | Apple M1 Pro, 10 cores (8 performance + 2 efficiency), 32 GB RAM |
| OS | macOS 26.6.2 (25G83) |
| Toolchain | Rust 1.99.1 (`rustc 1.99.1 (58cf8f9c3 2026-10-13)`), from `rust-toolchain.toml` |
| Linker | Apple ld `ld-27037.1` (ld-prime, Xcode), no custom linker |
| Profile | `dev` with the ARCHITECTURE §6 profile settings |
| Terminal | Terminal.app; Developer Tools registration (CONTRIBUTING.md) **not verified**: `spctl` has no status query and the setting lives behind TCC |
| Load | A normal desktop session, not an idle machine: WindowServer, a browser, the Claude app and Spotlight (`mds_stores`) were active. Load average was 7.8 before the first build |
| Sources | All crates fetched beforehand (`cargo fetch`), so no download time is included |

The crates are Phase 0 stubs, so these numbers are a baseline. The third-party set is complete, so clean-build numbers already reflect almost all of the dependency compile work. Rebuild and link times will grow as spektr's own code grows. The script that produced them is reproduced at the bottom; each scenario ran three times.

### Clean `cargo build`

`cargo clean`, then `cargo build --timings` (the default member, the `spektr` binary, which depends on all six library crates): 467 units, 376 crates compiled.

| Run | Wall | CPU (user) |
|---|---|---|
| 1 | 25.37 s | 169.04 s |
| 2 | 25.52 s | 168.41 s |
| 3 | 25.74 s | 168.79 s |

For comparison, ARCHITECTURE §6 measured 1 m 52 s on a 2-vCPU Linux sandbox.

### Rebuild after a one-line edit

Each run appends one line (`pub const TIMING_PROBE: u32 = N;`, with a different `N` each time) to the crate's `lib.rs`, then runs `cargo build --timings`. The file is restored afterwards.

| Edited crate | Run 1 | Run 2 | Run 3 | Recompiled |
|---|---|---|---|---|
| `spektr-tui` | 0.33 s | 0.33 s | 0.33 s | spektr-tui, spektr |
| `spektr-modules` | 0.34 s | 0.33 s | 0.34 s | spektr-modules, spektr |
| `spektr-engine` | 0.37 s | 0.37 s | 0.38 s | spektr-engine, spektr-modules, spektr-tui, spektr |
| `spektr-model` | 0.43 s | 0.43 s | 0.42 s | spektr-model, spektr-store, spektr-engine, spektr-modules, spektr-tui, spektr |

Times are cargo's `Finished … in` value. No third-party crate recompiled in any run, which matches the "What each edit recompiles" table in ARCHITECTURE §2. (`spektr-protocol` doesn't depend on `spektr-model`, so a model edit doesn't recompile it.)

### Link time of the binary

- **From `--timings`:** on Rust 1.99.1, cargo's timing report gives `frontend`/`codegen` sections for library units, but no `link` section for the binary unit (`"sections": None`). Its total, which includes linking, is the closest number the report gives: **0.12–0.13 s** in every one of the 15 reports above.
- **Measured directly:** the linker command was captured with `cargo rustc -p spektr --bin spektr -- -C save-temps --print link-args`, then re-run five times under `/usr/bin/time -p`: **0.06 s each time**. The binary is 3.2 MB (`target/debug/spektr`, 3,166,296 bytes).

Linking is about 18% of the `spektr-tui` rebuild (0.06 s of 0.33 s), below the 40% threshold in ARCHITECTURE §6, so no alternative linker was tried. Re-check this as the binary grows.

### Raw output

```
== clean builds
clean #1 | 25.37s | real 25.45s | user 169.04s | 376 crates compiled: …
clean #2 | 25.52s | real 25.59s | user 168.41s | 376 crates compiled: …
clean #3 | 25.74s | real 25.81s | user 168.79s | 376 crates compiled: …
== one-line edits
edit spektr-tui #1 | 0.33s | real 0.38s | user 0.24s | 2 crates compiled: spektr-tui spektr
edit spektr-tui #2 | 0.33s | real 0.37s | user 0.24s | 2 crates compiled: spektr-tui spektr
edit spektr-tui #3 | 0.33s | real 0.38s | user 0.24s | 2 crates compiled: spektr-tui spektr
edit spektr-modules #1 | 0.34s | real 0.39s | user 0.26s | 2 crates compiled: spektr-modules spektr
edit spektr-modules #2 | 0.33s | real 0.38s | user 0.24s | 2 crates compiled: spektr-modules spektr
edit spektr-modules #3 | 0.34s | real 0.38s | user 0.25s | 2 crates compiled: spektr-modules spektr
edit spektr-engine #1 | 0.37s | real 0.42s | user 0.28s | 4 crates compiled: spektr-engine spektr-modules spektr-tui spektr
edit spektr-engine #2 | 0.37s | real 0.41s | user 0.27s | 4 crates compiled: spektr-engine spektr-tui spektr-modules spektr
edit spektr-engine #3 | 0.38s | real 0.42s | user 0.27s | 4 crates compiled: spektr-engine spektr-tui spektr-modules spektr
edit spektr-model #1 | 0.43s | real 0.47s | user 0.30s | 6 crates compiled: spektr-model spektr-store spektr-engine spektr-tui spektr-modules spektr
edit spektr-model #2 | 0.43s | real 0.47s | user 0.29s | 6 crates compiled: spektr-model spektr-store spektr-engine spektr-tui spektr-modules spektr
edit spektr-model #3 | 0.42s | real 0.47s | user 0.30s | 6 crates compiled: spektr-model spektr-store spektr-engine spektr-modules spektr-tui spektr
```

(The clean-build crate lists are elided; each run listed the same 376 crates in a different order.)

Link timing:

```
$ for i in 1 2 3 4 5; do /usr/bin/time -p zsh -c "$(cat linkcmd.txt)" 2>&1 | grep real; done
real 0.06
real 0.06
real 0.06
real 0.06
real 0.06
```

`--timings` binary-unit durations (one line per report; the first is clean build #3):

```
20261020T063306 467 units | … spektr(bin)=0.13s
20261020T063332 2 units | spektr-tui=0.03s spektr(bin)=0.12s
20261020T063332 2 units | spektr-tui=0.04s spektr(bin)=0.12s
20261020T063333 2 units | spektr-tui=0.04s spektr(bin)=0.12s
20261020T063333 2 units | spektr-modules=0.04s spektr(bin)=0.13s
20261020T063334 2 units | spektr-modules=0.04s spektr(bin)=0.12s
20261020T063334 2 units | spektr-modules=0.04s spektr(bin)=0.12s
20261020T063334 4 units | spektr-engine=0.04s spektr-modules=0.04s spektr-tui=0.04s spektr(bin)=0.12s
20261020T063335 4 units | spektr-engine=0.04s spektr-modules=0.04s spektr-tui=0.04s spektr(bin)=0.12s
20261020T063335 4 units | spektr-engine=0.04s spektr-modules=0.04s spektr-tui=0.04s spektr(bin)=0.13s
20261020T063336 6 units | spektr-model=0.04s spektr-store=0.04s spektr-engine=0.04s spektr-modules=0.04s spektr-tui=0.04s spektr(bin)=0.12s
20261020T063336 6 units | spektr-model=0.04s spektr-store=0.03s spektr-engine=0.03s spektr-modules=0.04s spektr-tui=0.04s spektr(bin)=0.12s
20261020T063337 6 units | spektr-model=0.04s spektr-store=0.03s spektr-engine=0.03s spektr-modules=0.04s spektr-tui=0.04s spektr(bin)=0.12s
```

### Script

```zsh
measure() { # label
  local out
  out=$( { /usr/bin/time -p cargo build --timings 2>&1 } 2>&1 )
  print -r -- "$1 | $(print -r -- "$out" | grep Finished | sed 's/.* in //') | real … | user … | …"
}
for i in 1 2 3; do cargo clean -q; sleep 5; measure "clean #$i"; done
for crate in spektr-tui spektr-modules spektr-engine spektr-model; do
  f=crates/$crate/src/lib.rs; cp $f $f.bak
  for i in 1 2 3; do
    cp $f.bak $f; print "pub const TIMING_PROBE: u32 = $i;" >> $f
    measure "edit $crate #$i"
  done
  mv $f.bak $f
done
```

CI also uploads the `cargo build --timings` report from `aarch64-apple-darwin` on every run (artifact `cargo-timings-aarch64-apple-darwin`). Those builds are warm, using `Swatinem/rust-cache`, so they aren't comparable with the clean numbers above.

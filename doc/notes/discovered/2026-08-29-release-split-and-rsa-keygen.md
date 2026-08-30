# Why the release-build split failed, and where the test CPU went

*2026-08-29*

Two findings from one pass over CI. They are unrelated in mechanism and
related in cause: both are a default that is correct in a context nobody was
in.

## 1. The `-lduckdb` failure that reverted the release split

[#28](https://github.com/DataZooDE/peacock/pull/28) split `cargo build
--release` into its own job; [#29](https://github.com/DataZooDE/peacock/pull/29)
put it back, because the split job link-failed:

```
rust-lld: error: unable to find library -lduckdb
```

#29's own account of it was:

> That persisted even after adding the same `Ensure libduckdb is present`
> guard the ci job carries — the guard step reported success and the link
> still failed, so the guard is not sufficient and my model of why is wrong.
> The download lives under `target/duckdb-download/`, shared per cache scope;
> a second scope has to re-establish it and evidently does not.

The cache scope was a red herring. **`cargo clean -p <pkg>` cleans the dev
profile only** — it needs `--release` to touch `target/release`. Measured on
escurel's workspace with both profiles built:

```
$ cargo clean -p libduckdb-sys --dry-run -v | grep -oE 'target/(debug|release)' | sort | uniq -c
     58 target/debug

$ cargo clean -p libduckdb-sys --release --dry-run -v | grep -oE 'target/(debug|release)' | sort | uniq -c
     48 target/release
```

So the guard ran, reported success, and cleaned a profile that job never
builds. The stale release fingerprint survived, so libduckdb-sys did not
re-run, so it did not re-download, so the link had nothing to find. In the
`ci` job the bare command matches that job's profile, which is why the same
line worked there and looked like it should work here.

Root-caused and verified in escurel
([#428](https://github.com/DataZooDE/escurel/pull/428)), whose warm-cache run
shows the cache restored *and* the guard firing, then linking clean.

**The failure cannot appear on a first run.** A cold cache always links, so
the first run after any key change is green regardless. Verify a cache change
with two runs: one to seed the cache, then a commit touching no `Cargo.toml`,
`Cargo.lock` or `rust-toolchain.toml` so the key is unchanged and the second
run restores it. #29 also records that #28 was "merged while its CI was still
running" — the two mistakes compound, and the second is what let the first
reach main.

## 2. RSA keygen was most of the test CPU

peacock's no-mock tests spawn a real escurel gateway through
`escurel-test-support` (52 sites via `NorthwindEscurel`), and each spawn mints
a 2048-bit RSA keypair for the harness's in-process OIDC issuer. Tests build
with the `dev` profile, where dependencies are unoptimized, and unoptimized
that keygen measures a **mean of 4.88s**. At `opt-level = 3` it is 0.23s.

Measured on this workspace, warm tree, 32 cores:

| | before | after |
|---|---|---|
| `cargo nextest run --workspace` | 53.4s / 6m50s CPU | **14.1s / 2m01s CPU** |

Note that **escurel having this fix does nothing for us**: profile settings
apply only from the root of the workspace being built. Every consumer of
`escurel-test-support` needs its own copy of the four lines.

## 3. And while measuring: 15.3 GB of test binaries

peacock had no `[profile.dev]` debuginfo settings, so every test binary
embedded a full copy of the dependency graph's DWARF — **48 binaries, 15.3 GB,
averaging 490 MB**, with `peacock-core` alone at 8.8 GB. CI never paid for it
(it sets `CARGO_PROFILE_*_DEBUG=0`); local development paid all of it.

`debug = "line-tables-only"` for us, `debug = false` for dependencies, plus
consolidating `peacock-core` / `peacock-server` / `peacock-bin` into single
`tests/suite/` targets: **48 binaries -> 27, 15.3 GB -> 0.85 GB**.

## Verifying the split

A cache fix cannot be verified by the run that ships it: the first run after
any key change is cold, and a cold cache always links. Verification took two
runs on the same PR branch, the second commit touching no `Cargo.toml`,
`Cargo.lock` or `rust-toolchain.toml` so the rust-cache key was unchanged:

| run | cache | `build --release` |
|---|---|---|
| 1 | cold (`No cache found`, saved 664 MB) | 3m57s |
| 2 | restored | see the PR |

## How to recognise these next time

- **`cargo clean -p` takes a profile.** If a cleanup step runs in a job that
  builds `--release`, it needs `--release`. `--dry-run -v` prints exactly what
  would be removed.
- **A conditional step that prints nothing on the happy path hides a no-op.**
  "The guard reported success" and "the guard did nothing" look identical.
- **Cache bugs only reproduce on the second run.** Never merge a cache change
  on a first-run green.
- **A dependency doing real computation in a test build is unoptimized.**
  Crypto, compression, bignum — check for `[profile.dev.package.<dep>]`.
- **Profile settings do not cross workspace roots.** A fix in a path
  dependency's own manifest does not apply to you.

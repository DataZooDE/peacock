# How we work on peacock

This file is the working contract between contributors (including AI
assistants) and this codebase. Read it before opening a PR. It is the
peacock sibling of escurel's `CLAUDE.md`; it captures *how* we turn the
spec under [`doc/`](doc/) (`BRD.md`, `HLD.md`, `paper.md`) into running
code, not a re-statement of the spec.

peacock is the stateless **report renderer + MCP-App host** of the open
agent-reporting architecture (escurel · peacock · triton). It compiles an
escurel *report skill* + parameters into one artifact — A2UI v0.9 layout +
anofox-visualization charts (SVG, with a derived Vega-Lite for native-chart
hosts) + structuredContent + on-demand PNG — and serves it on
four surfaces (MCP App, chat-via-Triton, structured, embedded library). It
holds **no database credentials**; escurel is the only data path.

## Principles

1. **Red → green TDD.** Every code change starts with a failing test that
   names the target behaviour. No code without a test that would have
   caught its absence. Order is non-negotiable: red first, green second,
   refactor third.

2. **A task is done when a no-mock integration test passes locally.** Unit
   tests are fine for the inner loop. The merge gate during bootstrap is an
   integration test that exercises the **real** component — real escurel
   (`EscurelProcess::spawn`), real Triton (`TritonProcess::spawn`), real
   DuckDB, real Parquet, real anofox rendering, a real browser for the
   iframe (`scripts/verify-ui.sh`: rodney → headless Chrome).
   **No `FakeEscurel`, no `mockall`, no test doubles at the boundary the
   test exists to cover.** If you cannot exercise the real component from a
   test, the test is not finished. (This is the project owner's explicit
   directive and supersedes HLD §8.6's FakeEscurel suggestion.)

3. **Trust boundary is sacred.** peacock holds no DSN, no DB driver, no
   credential, and its **render path constructs no SQL string**. All data is
   ACL-checked rows from escurel via `escurel-client`. Untrusted params
   travel as typed values and are bound by escurel as prepared-statement
   parameters. The one place peacock touches SQL text is author time:
   `peacock author compile` (in `peacock-bin`) rewrites an author's anofox
   `dashboard.sql` into escurel query pages; the render crates only read
   those pages by reference. Static tests (grep) assert no SQL-construction
   / credential surface in peacock and no SQL planning or building in the
   render crates (ACC-2, ACC-11).

4. **One render core; surfaces are thin shells.** `(report skill, params,
   rows) → artifact` is the single pivot every surface funnels through
   (FR-R-1). Parity across MCP-App/chat/structured/embedded is by
   construction and is regression-guarded by a parity test (ACC-1).

5. **Statelessness.** No persistence, no server-side UI session. A drill is
   a fresh render from the **absolute** parameter vector. Renders are
   reproducible from `(report, params)` and from the audit log (ADR-P7).

6. **12-factor + substrate alignment.** Config via `PEACOCK_*` env (over
   CLI/defaults); JSON logs + one `kind: "audit"` line per render to stdout,
   written by the render core itself, failures included (never
   rows/tokens/param values/PII — param names only); `/healthz`, `/version` (binary + image + bundle SHA),
   tailnet-only `/metrics`; graceful SIGTERM drain; secrets are Vault refs
   in production (boot refuses literals).

7. **SOLID + clean code.** Boundaries are traits; one crate per concern;
   small, well-named public APIs. Crates: `peacock-types` (artifact, params,
   errors, principal, manifest), `peacock-theme` (CSS `--pk-*` tokens, host
   ⊕ brand composition), `peacock-core` (render core: resolve · read ·
   compose · guardrails, with an optional wire-trace sink; `chart` wraps the
   anofox-visualization engine — role specs → SVG → PNG, the neutral chart
   summary (`_meta.chart`), the deprecated derived Vega-Lite — and
   `dashboard` renders compiled anofox dashboards),
   `peacock-server`
   (surfaces + obs + lifecycle), `peacock-bin` (CLI/settings/manifest/signals,
   `author validate|scaffold|preview|compile`),
   `peacock-test-support` (`PeacockProcess` + Northwind fixtures + scenario
   reports), `peacock-demo` (the Copilot-style verification demo).

8. **Incremental PRs, ask don't assume.** One logical change per PR; when
   the spec is ambiguous or a cross-repo dependency is missing, raise it as
   a question rather than picking.

9. **Future-notes for discovered problems.** When a non-obvious problem is
   fixed (an anofox rendering quirk, a DuckDB parquet-dir gotcha, a Flutter
   semantics-label trap), write a short note under
   [`doc/notes/discovered/`](doc/notes/discovered/) as `<YYYY-MM-DD>-<slug>.md`:
   symptom, fix, how to recognise it next time. Don't rediscover twice.

## Pre-push gate (all four must pass)

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets` + `cargo test --workspace --doc`
  (what CI runs; `--all-targets` excludes doctests, hence the second command).
  `cargo nextest run --workspace` covers the same tests and is faster on a
  many-core machine, but slower on the 2-core `ubuntu-latest` runner CI uses —
  see `.config/nextest.toml` for the measurements.
- `cargo build --workspace --release`

### Test binaries live in `tests/suite/`

`peacock-core`, `peacock-server` and `peacock-bin` build **one** test binary
each: their tests sit in `tests/suite/` and are declared as modules from
`tests/suite/main.rs`. **If you add a test file to one of those crates, put it
in `tests/suite/` and add its `mod` line** — a file that is not listed there
is silently never compiled, and nothing warns you.

The layout matters: it must be `tests/suite/main.rs`, not `tests/suite.rs`. A
test target's root file resolves `mod x;` against its *own* directory.

Why: each of those binaries statically links the escurel gateway and libduckdb
at ~490-620 MB. Before this, the workspace built 48 test binaries totalling
15.3 GB; it now builds 27 totalling 0.85 GB. The smaller crates
(`peacock-types`, `peacock-theme`) are
deliberately untouched — at 7-41 MB a binary the churn and the lost process
isolation buy nothing. See
[`doc/notes/discovered/2026-08-29-release-split-and-rsa-keygen.md`](doc/notes/discovered/2026-08-29-release-split-and-rsa-keygen.md).

## Cross-repo dependencies (coordination)

peacock path-depends on two sibling repos under `/home/jr/Projects/datazoo`
(escurel, triton) and git-depends on a third:

- **anofox-visualization** — the chart engine (issue #44), a git dependency
  pinned by CalVer tag in the workspace `Cargo.toml` (not on crates.io; no
  features — peacock uses only the pure plan/render/theme/lint core, never
  anofox's own server or SQL execution). Upgrading is an explicit PR that
  bumps the tag.

- **escurel** — `escurel-client` (resolve/expand/**`query_instance`**),
  `escurel-types`, `escurel-test-support` (`EscurelProcess`,
  `FixtureBuilder`), path-depped at `../escurel`. `query_instance(ref,
  params)` merged to escurel `main` (PR #205/#206, commit `2f92201`); the
  former `feature/query-instance` worktree is gone. The data path: a
  `[[query::id]]` page with `target: [[sql_view_instance]]`, `{{target}}`
  substituted with the allow-listed managed view id, `:params` bound as
  prepared statements.
- **triton** — `triton-tests` (`TritonProcess`, `FakeConsul/Vault`).
  Upstream contract: Triton dispatches `render_report` to peacock as
  `POST /` + `X-Triton-Tool` + `Authorization: Bearer` + args body;
  peacock replies `2xx` with canonical A2UI `{surface:{components:[…]}}`.
  Register via `TRITON_STATIC_UPSTREAMS=render_report=host:port`.
  MCP-Apps proxying (`resources/read` of `ui://`,
  `callServerTool`/`updateModelContext` relay) and PNG delegation to peacock
  — specified in [`doc/triton-mcp-apps-proxying-issue.md`](doc/triton-mcp-apps-proxying-issue.md)
  — **landed in triton (#143)**, so the Triton-proxied path is now exercised
  end-to-end against the real `triton` binary in
  `crates/peacock-server/tests/suite/triton_upstream.rs` (no longer `#[ignore]`d).
  Triton passes peacock's tool-result `_meta` through MCP by an allow-list
  (triton#375/#383): `ui` to every client; `png_base64`, `svgs` and the
  neutral chart summary `_meta.chart` (contract §4b) to card hosts (MCP Apps
  clients) only. Triton draws native charts from `_meta.chart` only (the
  derived `_meta.vega_specs` was removed with triton#382). Peacock never
  sets `trace_id` or `triton/*` keys.
  peacock's MCP-App surface is also tested directly against peacock's own real
  `/mcp` endpoint. The `peacock-demo` binary spawns a real `TritonProcess` so
  the demo's "under the hood" inspector can show a genuine captured
  Triton→peacock dispatch (degrades gracefully if the `triton` binary is
  absent).

## Running example (the demonstration thread)

The paper's **Northwind monthly revenue by product category** is the
end-to-end demo, seeded from `fixtures/northwind/*.parquet` via escurel's
credential-free offline `parquet_dir` connector → a `sql_view` aggregation
view → a `[[query::nw_revenue_by_category]]` query page read by
`query_instance`. Every surface renders this report.

## Browser verification (Flutter web iframe)

The iframe runtime (`web/peacock-web`, Flutter web) renders to a CanvasKit
`<canvas>` — there is **no CSS-selectable DOM** for its widgets. Drive it
through the **semantics (accessibility) tree** (force-enable semantics at
startup; every interactive widget carries a stable `Semantics(label: …)` —
those labels are the selector contract). Browser-driven tests use the
chrome-devtools MCP against the **peacock-served** bundle. (Mirrors
escurel's `apps/escurel-explore` rodney approach.)

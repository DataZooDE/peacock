# How we work on peacock

This file is the working contract between contributors (including AI
assistants) and this codebase. Read it before opening a PR. It is the
peacock sibling of escurel's `CLAUDE.md`; it captures *how* we turn the
spec under [`doc/`](doc/) (`BRD.md`, `HLD.md`, `paper.md`) into running
code, not a re-statement of the spec.

peacock is the stateless **report renderer + MCP-App host** of the open
agent-reporting architecture (escurel · peacock · triton). It compiles an
escurel *report skill* + parameters into one artifact — A2UI v0.9 layout +
Vega-Lite charts + structuredContent + on-demand PNG — and serves it on
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
   DuckDB, real Parquet, real vl-convert, a real browser for the iframe.
   **No `FakeEscurel`, no `mockall`, no test doubles at the boundary the
   test exists to cover.** If you cannot exercise the real component from a
   test, the test is not finished. (This is the project owner's explicit
   directive and supersedes HLD §8.6's FakeEscurel suggestion.)

3. **Trust boundary is sacred.** peacock holds no DSN, no DB driver, no
   credential, and constructs **no SQL string**. All data is ACL-checked
   rows from escurel via `escurel-client`. Untrusted params travel as typed
   values and are bound by escurel as prepared-statement parameters. A
   static test (grep) asserts no SQL-construction / credential surface
   exists in peacock (ACC-2, ACC-11).

4. **One render core; surfaces are thin shells.** `(report skill, params,
   rows) → artifact` is the single pivot every surface funnels through
   (FR-R-1). Parity across MCP-App/chat/structured/embedded is by
   construction and is regression-guarded by a parity test (ACC-1).

5. **Statelessness.** No persistence, no server-side UI session. A drill is
   a fresh render from the **absolute** parameter vector. Renders are
   reproducible from `(report, params)` and from the audit log (ADR-P7).

6. **12-factor + substrate alignment.** Config via `PEACOCK_*` env (over
   CLI/defaults); JSON logs + one audit line per render to stdout (never
   rows/tokens/PII); `/healthz`, `/version` (binary + image + bundle SHA),
   tailnet-only `/metrics`; graceful SIGTERM drain; secrets are Vault refs
   in production (boot refuses literals).

7. **SOLID + clean code.** Boundaries are traits; one crate per concern;
   small, well-named public APIs. Crates: `peacock-types` (artifact, params,
   errors, principal, manifest), `peacock-core` (render core: resolve ·
   read · compose · guardrails · rasterize), `peacock-server` (surfaces +
   obs + lifecycle), `peacock-bin` (CLI/settings/manifest/signals),
   `peacock-test-support` (`PeacockProcess` + Northwind fixtures).

8. **Incremental PRs, ask don't assume.** One logical change per PR; when
   the spec is ambiguous or a cross-repo dependency is missing, raise it as
   a question rather than picking.

9. **Future-notes for discovered problems.** When a non-obvious problem is
   fixed (a vl-convert quirk, a DuckDB parquet-dir gotcha, a Flutter
   semantics-label trap), write a short note under
   [`doc/notes/discovered/`](doc/notes/discovered/) as `<YYYY-MM-DD>-<slug>.md`:
   symptom, fix, how to recognise it next time. Don't rediscover twice.

## Pre-push gate (all four must pass)

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `cargo build --workspace --release`

## Cross-repo dependencies (coordination)

peacock path-depends on two sibling repos under `/home/jr/Projects/datazoo`:

- **escurel** — `escurel-client` (resolve/expand/**`query_instance`**),
  `escurel-types`, `escurel-test-support` (`EscurelProcess`,
  `FixtureBuilder`). `query_instance(ref, params)` currently lives on the
  **`feature/query-instance`** worktree
  (`../escurel-query-instance`, **uncommitted**). The data path: a
  `[[query::id]]` page with `target: [[sql_view_instance]]`, `{{target}}`
  substituted with the allow-listed managed view id, `:params` bound as
  prepared statements. **Repoint the path deps to `../escurel/...` once
  `query_instance` merges to escurel `main`.**
- **triton** — `triton-tests` (`TritonProcess`, `FakeConsul/Vault`).
  Upstream contract: Triton dispatches `render_report` to peacock as
  `POST /` + `X-Triton-Tool` + `Authorization: Bearer` + args body;
  peacock replies `2xx` with canonical A2UI `{surface:{components:[…]}}`.
  Register via `TRITON_STATIC_UPSTREAMS=render_report=host:port`.
  **NOT YET BUILT in triton:** MCP-Apps proxying (`resources/read` of
  `ui://`, `callServerTool`/`updateModelContext` relay) and PNG delegation
  to peacock — specified in [`doc/triton-mcp-apps-proxying-issue.md`](doc/triton-mcp-apps-proxying-issue.md).
  peacock's MCP-App surface is built & tested directly against peacock's own
  real `/mcp` endpoint; the Triton-proxied variant is `#[ignore]`d pending
  that issue.

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

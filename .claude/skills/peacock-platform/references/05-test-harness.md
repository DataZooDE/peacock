# 05 — The no-mock test harness

Peacock's merge gate is a no-mock integration test against **real**
components (`CLAUDE.md` principle 2): real escurel, real DuckDB /
Parquet, real vl-convert, the real `triton` binary where the wire
matters. Consumers get the same harness.

## `peacock-test-support`

```toml
[dev-dependencies]
peacock-test-support = { path = "…/peacock/crates/peacock-test-support" }
```

- **`PeacockProcess::spawn(env: HashMap<String,String>)`** — the real
  `peacock` binary on a free port. Minimum env:
  `PEACOCK_ESCUREL_URL`; useful knobs: `PEACOCK_TENANT`, `PEACOCK_SUB`,
  `PEACOCK_ESCUREL_TOKEN`, `PEACOCK_BRAND_CSS`. `base_url()` +
  `terminate()`.
- **`NorthwindEscurel::spawn()` / `spawn_with(NorthwindOpts)`** — a
  real escurel seeded with the Northwind fixtures (parquet →
  `sql_view` → query pages → the demo report). `NorthwindOpts` adds
  your own pages: `extra_skills: Vec<(skill_id, markdown)>`,
  `extra_instances: Vec<(skill_id, instance_id, markdown)>`, plus
  gateway `config_overrides` (e.g. `webhook_url`/`webhook_secret` for
  event-chain tests). Principals: `sales_principal()` /
  `no_sales_principal()` (ACL fail-closed tests),
  `mint_token_with_groups(…)`, `sales_client()` (an escurel-client for
  direct verification — inbox contents, page state).
- `NW_REPORT` — the seeded demo report id.

## Patterns worth copying (from peacock's own suites)

- **In-proc server state** for surface tests: build an
  `AppState { escurel: EscurelData::new(endpoint), principal, themes, … }`
  and `serve(addr, state)` — lets you register a test brand
  (`ThemeRegistry::register_brand`) without a binary boot.
  (`crates/peacock-server/tests/mcp_surface.rs`.)
- **Through-Triton tests**: `triton_tests::TritonProcess::spawn_with_env`
  with `TRITON_STATIC_UPSTREAMS=render_report=<peacock>,…` proves the
  proxied chain against the real gateway binary.
  (`crates/peacock-server/tests/triton_upstream.rs`.)
- **Fail-closed assertions belong in every consumer suite**: smuggled
  instance ids → Validation before any read; a group-gated skill +
  outsider principal → no partial artifact; unknown/prompt-named
  `emit_document_event` → error and NOTHING captured (check
  `list_inbox`).
- **Instance timelines need capture + assign** — an unassigned inbox
  event never renders in a `timeline` view.

## Gotchas

- The `triton` binary harness checks source mtimes: rebuilding the
  triton workspace concurrently with a peacock test run makes the
  binary look stale mid-run. Build first, then test.
- Escurel's search index builds asynchronously after seeding — gate
  first-turn assertions on a fixture page being findable, or drive by
  page id (`expand`) instead of search.

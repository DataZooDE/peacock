# 00 — What Peacock is

Peacock is the platform's **stateless report renderer + MCP-App host**
in the `escurel · peacock · triton` triangle:

```text
frontend / chat ──► TRITON (gateway: protocol adapters + auth)
                       │ dispatches render_report / emit_document_event / get_theme
                       │ proxies resources/read of ui://peacock/…
                       ▼
                    PEACOCK (render: reports, documents, PNGs, themes)
                       │ query_instance / resolve+expand / list_events / capture_event
                       ▼
                    ESCUREL (per-tenant knowledge + data + events, ACL at the boundary)
```

- **One render core.** `render(report_id, params, principal, escurel)`
  → an `Artifact { a2ui, vega_specs, structured_content, png? }`. Every
  surface funnels through it; a drill is a fresh render from the
  **absolute** param vector (no server-side UI state).
- **Four surfaces**: MCP App (`tools/call render_report` +
  `resources/read ui://peacock/<id>` → a self-contained iframe
  runtime), chat-via-Triton (`POST /` + `X-Triton-Tool`), structured
  HTTP (`POST /v1/render_report`), and the embedded library.
- **Trust boundary**: no DB credentials, no SQL construction. Rows come
  from escurel `query_instance` (authored query pages), pages from
  `resolve`+`expand`, history from `list_events` — all as the CALLER
  (the per-request principal's bearer is forwarded; escurel's
  fail-closed ACL applies). Peacock's single write path is
  `emit_document_event` → escurel `capture_event`, also as the caller.
- **Theming lives here** — see `references/04`. One CSS file of
  `--pk-*` tokens themes charts, iframes and chat-card chrome alike.

What renders: row reports (KPI / Vega-Lite chart / table over query
rows), instance reports (an escurel RECORD: frontmatter facts /
markdown / event timeline), and documents (any instance page via the
reserved `document` pseudo-report — `references/02`).

Everything is authored in escurel: adding a report, a query, or a
record type's document affordances is a **page write**, not a deploy.

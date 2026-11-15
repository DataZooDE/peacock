# Two boundaries met while moving charts to anofox

**Triton's MCP surface rebuilds `_meta`.** A `tools/call render_report` proxied through Triton's
MCP endpoint returns peacock's `structuredContent` but only Triton's own `_meta` (`trace_id`,
`ui`) — `png_base64`, `svgs` and `vega_specs` are dropped. They are not lost: Triton's chat and
A2A adapters read the *dispatch* result (visible whole via `POST /v1/tools/render_report`), which
is how the Teams card gets its native `Chart.*` (`find_vega_spec` → `chart_from_vega`). Assert on
the dispatch result, not the MCP `_meta`, when testing chat inputs through Triton.

**anofox's single-panel API does not draw tables or records from JSON rows.** `TABLE` and
`RECORD` panels expect DuckDB-typed results (structs/maps) and render empty from
`render_spec_checked({rows, roles})`. The instance-card preview therefore uses a `MARKDOWN` panel
under a `TITLE` bar (static markdown is plain text: list lines and paragraphs, no bold), and the
KPI tiles use `TEXT_LARGE` with a `LABEL` caption.

# 03 — Integration wiring (Triton, agents, frontends)

## Triton registration — the four keys

Peacock is a **sibling Triton upstream**. The operator registers it in
the same static map as the agent, under its tool names AND its `ui://`
authority:

```text
TRITON_STATIC_UPSTREAMS=assistant=<agent>:8080,\
                        render_report=<peacock>:8080,\
                        emit_document_event=<peacock>:8080,\
                        get_theme=<peacock>:8080,\
                        peacock=<peacock>:8080
```

- `render_report` / `emit_document_event` / `get_theme` — tool
  dispatch (`POST /` + `X-Triton-Tool: <name>`).
- `peacock` — the `ui://` authority: Triton proxies `resources/read`
  (and the iframe's `callServerTool` re-renders) to it.
- Optional: `TRITON_RASTERIZE_UPSTREAM=render_a2ui_to_png` delegates
  Triton's dashboard rasterisation here (peacock renders the PNG;
  Triton transports it).

Identity: Triton mints the per-call bearer. Peacock's own escurel
principal is deployment config (`PEACOCK_TENANT`/`PEACOCK_SUB`/
`PEACOCK_ESCUREL_TOKEN`) — an agent never forwards its bearer to
peacock.

## The reference pattern (what an agent emits)

An agent puts a report **reference** in its surface — never a call:

```json
{ "kind": "report", "report_id": "<id>", "args": { "params": {…} } },
{ "kind": "button", "label": "Open report: <id>",
  "tool": "render_report",
  "args": { "report_id": "<id>", "params": {…} },
  "resource": "ui://peacock/<id>?<urlencoded scalar params>" }
```

- The **params ride the resource URI** (scalars, urlencoded) so the
  served runtime's FIRST render is self-sufficient — required for
  param-mandatory reports (a customer record, a briefing).
- Capable hosts (the Explorer) **auto-open** the first resource-bearing
  button's app inline; image-hosting chat adapters expand the inline
  `report` into the chart with zero clicks.

## The tool result (what `render_report` returns)

```json
{ "content": [{ "type": "text", "text": "Rendered `<id>` — N rows…" }],
  "structuredContent": { "rows", "param_schema", "current_params",
                          "instances?", "document?", "truncated?" },
  "isError": false,
  "_meta": { "ui": { "resourceUri": "ui://peacock/<id>?…" },
             "png_base64": "…", "svgs": ["<svg…>"],
             "chart": { "kind": "bar", "x": "month", "y": "Revenue",
                        "series": "category", "stacked": true,
                        "orientation": "vertical", "title": "…",
                        "rows": [ … ] } } }
```

- `_meta.chart` — the renderer-neutral summary of the **primary** (first)
  chart, Triton's upstream contract §4b (`chart` v1, triton#375). Triton maps
  it to a channel's native chart (Teams `Chart.*`, Gemini Enterprise
  `VegaChart`). Derived from the anofox role spec: `XAXIS` → `x`, the value
  role → `kind` + `y`, `CATEGORY` → `series` (pie/donut: `x`), `TITLE` /
  `XTITLE` / `YTITLE` / `YFORMAT` (or `CURRENCY_CODE`) → `title` / `x_title` /
  `y_title` / `unit`, `FLIP` → `orientation: horizontal`, `BARCHART_STACKED`
  with a series → `stacked: true`. At most 200 rows; points with a null or
  non-numeric `y` are dropped. Absent when no chart fits (heatmap, histogram,
  a second measure, …) — Triton then shows the PNG.
- `_meta.vega_specs` — **removed** (triton#382). Neither peacock nor Triton
  uses it any more.
- `structuredContent.truncated` — `{ rows_shown, rows_total?, limit, view }`
  when a row cap clipped a view (escurel's server cap, 10 000, which reports
  no total; or peacock's own `max_rows`). The text content says so too, the
  `ui://` runtime shows a "Partial data" badge, and the capped view's chart
  title (in the painted SVG/PNG and in `_meta.chart.title`) gains
  `(Partial data: first N of M rows)` / `(Partial data: first N rows; more
  exist)`. The title is the only render text a Triton chat card shows: for
  an inline `report` component Triton builds the card from the agent's own
  surface plus `_meta.chart` (or the PNG), never from peacock's text content.
  Absent otherwise.
- Peacock never sets `trace_id`, a `triton/*` key or an MCP-reserved key in
  `_meta`. Through Triton's MCP ingress, peacock's `_meta` passes by an
  allow-list with audiences (triton#383): `ui` reaches every client;
  `chart`, `png_base64` and `svgs` reach **card hosts** only
  (clients that declared the MCP Apps extension). An operator can widen or
  drop a key per tool in Triton's `adapter.yaml` (`tools.render_report.meta`).

A drill is a fresh `tools/call render_report` with the new ABSOLUTE params.

## The audit line (one per render, stdout)

Every render writes exactly one JSON line with `kind: "audit"` to stdout
(FR-O-4), beside the `tracing` JSON log, on every surface — `mcp`
(`/mcp` `tools/call`), `http` (`/v1/render_report`), `triton` (`POST /`
upstream), `document_page`, and `embedded` (a library caller of
`peacock_core::render`; set `RenderOpts::audit` to `None` to opt out, as
the author CLI preview does). Failed and refused renders are audited too.

```json
{"kind":"audit","event":"render","when":"2026-11-23T05:12:09.481Z",
 "surface":"mcp","tenant":"acme","who":"analyst-1",
 "report_id":"northwind-line-combinations","param_names":[],"result":"ok",
 "duration_ms":912,"rows":10000,
 "truncated":{"rows_shown":10000,"limit":10000,"view":"lines"}}
```

- `result`: `ok` or `error:<auth|validation|data|render>`.
- Optional: `host` (only a registered theme flavor), `trace_id`, `report_id`
  (absent if the request named none), `rows` (ok only), `truncated`.
- Never: rows, cell values, param VALUES (names only), error messages,
  tokens. The substrate collector ships the line; peacock ships nothing.

## The iframe host verbs

The served `ui://` runtime is a self-contained single file. Embedded,
it speaks three postMessage verbs to its host:

| Verb | Direction | Meaning |
|---|---|---|
| `mcp:callServerTool` `{reqId, name, arguments}` | app → host → Triton | data fetch / drill re-render / `emit_document_event`; host posts back `mcp:callServerTool:result` |
| `mcp:updateModelContext` `{record}` | app → host | compact view-state (`{report_id, params, salient_summary}`) for the model — never rows |
| `mcp:prompt` `{text}` | app → host | send `text` as a NEW USER TURN (document prompt actions) |

Standalone (no host), the runtime falls back to peacock's own HTTP
endpoints — a `ui://` resource is independently verifiable in a plain
browser tab.

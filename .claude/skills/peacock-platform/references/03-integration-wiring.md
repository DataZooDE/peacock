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
                          "instances?", "document?" },
  "isError": false,
  "_meta": { "ui": { "resourceUri": "ui://peacock/<id>?…" },
             "png_base64": "…" } }
```

Through Triton's MCP ingress this nests once more:
`result.structuredContent.result.{structuredContent,_meta}`. A drill is
a fresh `tools/call render_report` with the new ABSOLUTE params.

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

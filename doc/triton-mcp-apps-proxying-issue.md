# Triton feature request: MCP-Apps proxying + PNG-rasterization delegation for upstream renderers

> **UPDATE 2026-06-27 — parts A/B/C have LANDED in Triton (issue #143).** The
> current `crates/triton-adapters-http/src/mcp.rs` already (A) surfaces an
> upstream result's `_meta.ui` on the `tools/call` response, (B) proxies
> `resources/read` of a `ui://<authority>/…` URI to the owning upstream via
> `POST /` + header `X-Triton-MCP: resources/read`, body `{ "uri": … }`, and
> (C) relays `updateModelContext` via `X-Triton-MCP: updateModelContext`,
> body `{ uri, record }` (record verbatim). peacock implements that exact
> upstream contract (see `peacock-server`'s `POST /` handler) and its
> Triton-proxied tests are now enabled. **Part D (delegating chat dashboard
> rasterization to a registered `render_a2ui_to_png` upstream) is the
> remaining open item.**


> **Paste-ready.** Copy this file's body into a new issue on
> `github.com/DataZooDE/triton`. Verified against the current triton tree
> (`crates/triton-adapters-http`, `crates/triton-core`,
> `crates/triton-upstream`) on 2026-06-27.

## Summary

We have a new internal **upstream renderer** (peacock) that sits behind
Triton's MCP ingress. It needs four capabilities Triton does not yet
provide. peacock returns a `render_report` tool result that carries both
`structuredContent` **and** an [MCP-Apps](#background-mcp-apps) `ui://`
resource link, hosts an interactive iframe behind that `ui://` URI, and
handles in-iframe drills as `callServerTool` re-renders that also push a
compact view-state summary to the model via `updateModelContext`. peacock
is also a Vega-Lite → PNG rasterizer (`render_a2ui_to_png`) that Triton's
chat surface can delegate to.

Today Triton implements the upstream tool-call contract (`POST /` +
`X-Triton-Tool` + Bearer; canonical A2UI reply) but does **not**:

1. relay an upstream tool result's `_meta.ui.resourceUri` to the MCP host,
2. proxy `resources/read` of an upstream-owned `ui://` URI to that upstream,
3. relay `callServerTool` / `updateModelContext` interactions, or
4. delegate chat dashboard rasterization to a registered upstream.

`crates/triton-adapters-http/src/mcp.rs` `resources/read` currently serves
only the stub `ui://triton/runtime.html`; every other URI errors. This
issue asks for the four capabilities, scoped so any MCP-Apps upstream works
unchanged.

## Background: MCP-Apps

The MCP-Apps extension (SEP-1865) lets a server return a `ui://` HTML
resource that the MCP host renders in a sandboxed iframe. The iframe talks
back to the host over `postMessage` using two verbs: `callServerTool` (run
a server tool and re-render) and `updateModelContext` (push a compact
summary into the model's context). For this to work through Triton, Triton
must pass the resource link through, serve the resource on demand, and
relay those two verbs.

## Scope

### A. Pass through `_meta.ui.resourceUri` on upstream tool results

When an upstream tool result (e.g. `render_report`) includes
`_meta.ui.resourceUri` (and any sibling `_meta.ui.*` MCP-Apps fields),
Triton's MCP adapter MUST preserve it on the `tools/call` response it
returns to the host, alongside the existing `content` / `structuredContent`
/ `isError` / `_meta.trace_id` envelope.

- Today `tools/call` wrapping lives in
  `crates/triton-adapters-http/src/mcp.rs` (~L292–340). The wrap MUST NOT
  drop unknown `_meta.ui.*` keys produced by the upstream.
- The resource URI scheme/owner mapping (e.g. `ui://peacock/<report>` →
  the upstream that owns `render_report`) MUST be derivable so step B can
  route `resources/read` back to the owning upstream.

### B. Proxy `resources/read` of an upstream-owned `ui://` URI

`resources/read` for a `ui://<owner>/<...>` URI owned by a registered
upstream MUST proxy to that upstream and return its resource contents (the
iframe bundle / HTML) instead of erroring.

- Extend the `resources/read` handler in `mcp.rs` (~L343–365, currently
  only `ui://triton/runtime.html`).
- Routing: resolve `<owner>` → upstream endpoint via the same registry used
  for tool dispatch (static `TRITON_STATIC_UPSTREAMS` and/or Consul). The
  proxied request SHOULD reuse the upstream wire contract (a dedicated
  method/path is fine; see [Wire contract](#wire-contract-proposed)) and
  carry the same minted Bearer + principal as a `tools/call`.
- `resources/list` SHOULD include upstream-advertised resources (optional
  in v1; `resources/read` is the load-bearing call).

### C. Relay `callServerTool` and `updateModelContext`

- A host's in-iframe `callServerTool('render_report', {absolute params})`
  MUST dispatch to the owning upstream exactly like a normal `tools/call`
  (the upstream is stateless — params are absolute, never deltas).
- `updateModelContext` records pushed from the iframe MUST be relayed to
  the host's model-context channel unmodified. They are **compact**
  `{report_id, params, salient_summary}` payloads (no row data); Triton
  MUST NOT inspect or expand them.

### D. Delegate chat dashboard rasterization to a registered upstream

Triton's chat surface today rasterizes dashboards via the in-tree
`triton-rasterizer` sidecar. An upstream can instead expose a
`render_a2ui_to_png(spec|surface) -> png` tool (peacock embeds `vl-convert`,
no Node/network) and Triton can delegate to it.

- Add a manifest/config option to route chat dashboard rasterization to a
  registered upstream tool `render_a2ui_to_png` instead of (or as a
  fallback before) the local sidecar.
- Keep the sidecar as the default; this is opt-in per deployment so the
  change is non-breaking.

## Wire contract (proposed)

The upstream already serves a `POST /` endpoint for tool calls. For B/C,
propose Triton calls the upstream with explicit MCP-Apps verbs carried in
the existing header-routed shape, e.g.:

- `X-Triton-Tool: render_report` + body = tool args → tool result with
  `_meta.ui.resourceUri` (A, C).
- `X-Triton-MCP: resources/read` + body = `{ "uri": "ui://peacock/<report>" }`
  → `{ "contents": [{ "uri", "mimeType", "blob"|"text" }] }` (B).
- `X-Triton-MCP: updateModelContext` relayed host→model (C) — Triton-side
  passthrough; no upstream call needed if the host owns the channel.

Bearer minting, principal forwarding (`sub`/`tenant`/scopes), the SSRF
egress guard, the per-tool circuit breaker, and the single audit line per
dispatch MUST apply to these proxied calls identically to `tools/call`.
(The upstream will adapt to whatever final shape Triton picks; the above is
a concrete starting proposal.)

## Acceptance (real `TritonProcess` + a fake upstream)

1. A `tools/call render_report` whose result has
   `_meta.ui.resourceUri = ui://peacock/r1` returns that field intact to
   the host (A).
2. `resources/read ui://peacock/r1` proxies to the owning upstream and
   returns its bundle bytes; an unknown owner still errors (B).
3. `callServerTool('render_report', {…})` dispatches to the upstream as a
   fresh tool call; `updateModelContext` is relayed unmodified (C).
4. With the delegation option enabled, a chat dashboard render calls the
   registered upstream `render_a2ui_to_png` and embeds its PNG; with it
   disabled, the local sidecar is used (D).
5. Minted Bearer, principal forwarding, SSRF guard, circuit breaker, and
   audit line behave identically on the proxied paths.

## Non-goals

- No iframe runtime in Triton (the upstream owns its bundle).
- No A2UI authoring/composition in Triton (the upstream composes; Triton
  relays).
- No persistence / server-side UI session (renders stay stateless; state is
  the absolute parameter vector).

## Upstream-side status

peacock already implements and tests all four surfaces directly against its
own real endpoints (MCP `/mcp`, `POST /` upstream, `render_a2ui_to_png`), so
it does not block on this issue. peacock's Triton-proxied integration tests
are `#[ignore]`d with a comment linking here and will be enabled once this
lands.

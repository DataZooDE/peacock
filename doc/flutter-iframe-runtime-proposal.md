# Flutter as the peacock MCP-Apps `ui://` runtime

> Status: implemented on `track/flutter-fix`. This document is the proposal +
> the rationale for the code that landed with it.

peacock's interactive surface — the MCP-Apps `ui://peacock/<report>` resource a
host renders in a sandboxed iframe — should be the **Flutter-web** client under
`web/peacock-web`, not a separate HTML runtime. Two concrete problems stood in
the way. This doc states them, weighs the options, recommends one, and shows how
the result composes with Triton's MCP-Apps proxying.

## Problem 1 — subpath serving (`base-href`)

peacock serves the built bundle at `/app/` (`AppState.flutter_dir` →
`ServeDir`, `crates/peacock-server/src/http.rs`). A Flutter web `index.html`
resolves its engine and asset URLs (`flutter.js`, `main.dart.js`, `canvaskit/`,
`assets/`) **relative to the document's `<base href>`**. The Flutter default is
`<base href="/">`, which makes the browser fetch `/canvaskit/…` and `/assets/…`
at the **origin root** — but peacock serves them under `/app/…`, so they 404 and
the app never boots. Until now the only fix was to *remember* to pass
`--base-href /app/` at build time; forget it and the bundle silently breaks.
That coupling is fragile, and it gets worse once the bundle is also nested under
a host iframe at some other path.

### Options

| Option | How | Trade-off |
|---|---|---|
| **A. Build-time `--base-href /app/`** (status quo) | pass the flag every build | works only at exactly `/app/`; a forgotten flag = silent 404s; breaks if nested elsewhere |
| **B. Relative base (`<base href=".">`)** | build with `--base-href ./` | Flutter's bootstrap and service-worker assume an absolute base; `.` is brittle across deep paths and history navigation |
| **C. Runtime base-href rewrite** (recommended) | a tiny inline script rewrites `<base href>` to `window.location.pathname`'s directory before `flutter_bootstrap.js` runs | mount-agnostic — same bundle works at `/`, `/app/`, or any nested path; no build flag to remember |
| **D. Per-deployment templating** | server rewrites `<base>` on the fly | puts HTML rewriting in the serving path; more moving parts than C |

### Recommendation: C (runtime base-href) + a build check

`web/peacock-web/web/index.html` now carries an inline script that, before the
Flutter bootstrap loads, sets `<base href>` to the directory the document was
served from (`window.location.pathname`, trimmed to its trailing slash). The
bundle is therefore **mount-agnostic**: CanvasKit/assets resolve correctly at
`/app/` and under a nested host iframe, with no build flag dependency.

We keep `--base-href /app/` as a belt-and-braces static default (for no-JS
crawlers and to match the primary mount) and make it **verifiable** instead of
remembered: `web/peacock-web/build_web.sh` runs the build and asserts (1) the
static `<base href>` matches the mount, (2) the runtime rewrite is present, and
(3) the load-bearing engine files exist. CI/dev run that one script.

## Problem 2 — Flutter (multi-file) as a single `ui://` HTML resource

MCP-Apps (SEP-1865) hands the host **one** HTML resource (`resources/read` →
`{ contents: [{ mimeType: "text/html", text|blob }] }`) which it renders in a
sandboxed iframe. The iframe talks back over `postMessage` with two verbs:
`callServerTool(name, args)` (run a server tool, re-render) and
`updateModelContext(record)` (push a compact summary into the model's context).

A Flutter web build is **multi-file** — `index.html` + `flutter_bootstrap.js` +
`main.dart.js` + `flutter.js` + `canvaskit/` + `assets/`. It cannot be the
single inlined HTML resource. peacock's current `ui://` resource
(`crates/peacock-server/assets/iframe.html`) sidesteps this by being a
self-contained **HTML** runtime — not Flutter. We want Flutter to be the runtime
without giving up the single-resource contract.

### Options

| Option | How | Trade-off |
|---|---|---|
| **1. Base64-inline the whole bundle** | concatenate every file into one HTML via data: URLs / blob URLs | CanvasKit alone is multi-MB; inlining is huge, defeats caching, and the service-worker/asset-manifest paths assume real URLs — fragile and slow |
| **2. `flutter build web` HTML renderer** | drop CanvasKit, render to DOM | the HTML renderer is **removed** in current Flutter (CanvasKit/skwasm only); also loses the crisp parity with the chat surface |
| **3. Service-worker bootstrap** | a tiny HTML installs a SW that synthesizes the bundle | SWs are unreliable inside cross-origin sandboxed iframes (scope/registration limits); too much machinery for the host's threat model |
| **4. Shim that nests the hosted bundle** (recommended) | the `ui://` resource is a **tiny self-contained HTML shim** that loads peacock's hosted Flutter app (`/app/`) in a child `<iframe>` and **bridges** the MCP-Apps `postMessage` channel between the host and the Flutter app | one small inlinable resource; the multi-file bundle stays multi-file and cacheable, served from peacock's own origin; the Flutter app is the real runtime |

### Recommendation: 4 (nesting shim) + the bridge in Flutter

Two pieces, both landed here:

1. **The shim** (`crates/peacock-server/assets/flutter-shim.html`, served at
   `GET /app-shim?report=<id>` from `http.rs`). It is the single HTML resource a
   host's `ui://peacock/<report>` iframe loads. It nests `/app/#mcp&report=<id>`
   in a child iframe and is a **transparent relay**:
   - app → host: forwards `mcp:callServerTool` / `mcp:updateModelContext` up to
     `window.parent` (the host).
   - host → app: forwards `mcp:callServerTool:result` (matched by `reqId`) down
     to the Flutter iframe.
   - standalone (no host): answers `callServerTool` itself via peacock's
     same-origin `POST /v1/render_report`, so the shim is independently
     verifiable — mirroring `iframe.html`'s standalone branch.

2. **The Flutter bridge** (`web/peacock-web/lib/mcp_bridge*.dart`). The Flutter
   app detects its embedding at startup (`Mcp.detect()`): if it has a distinct
   parent window it uses the **host bridge** (`callServerTool` /
   `updateModelContext` over `postMessage`, via `dart:js_interop` +
   `package:web`); standalone (opened directly at `/app`) it uses the
   **same-origin `fetch('/v1/render_report')` fallback**. A committed drill
   publishes a compact `{report_id, params, salient_summary}` record via
   `updateModelContext` (no row data). The message shapes mirror
   `iframe.html` exactly:

   ```
   request : { type: "mcp:callServerTool",        reqId, name, arguments }
   result  : { type: "mcp:callServerTool:result", reqId, result }
   context : { type: "mcp:updateModelContext",    record }
   ```

The shim is loaded from peacock's `/app` origin, so the nested Flutter iframe is
**same-origin** with the shim — `postMessage` and relay work without
cross-origin friction, while the host sees only the one shim resource.

> **Why not change `mcp.rs` to return the shim?** `crates/peacock-server/src/mcp.rs`
> is intentionally out of scope for this change, so `resources/read` still
> returns the existing self-contained `iframe.html`. The Flutter path is fully
> in place behind it: serving the shim, the nested bundle, and the Dart bridge.
> The single remaining one-line cutover — have `resources_read` serve the shim
> (with the same `__REPORT_ID__` injection) instead of `iframe.html`, or 302 to
> `/app-shim?report=<id>` — is a follow-up PR scoped to `mcp.rs`. Until then
> `iframe.html` remains the inlined runtime and the Flutter app is reachable
> standalone at `/app` and via `/app-shim`.

## Why this is the right shape

- **Single-resource contract preserved.** The host still gets one small HTML
  resource; it never sees the multi-file bundle.
- **The bundle stays a bundle.** Cacheable, served by `ServeDir` from peacock's
  origin, no multi-MB inlining.
- **Statelessness preserved (HLD §5).** A drill is a fresh `callServerTool` with
  the **absolute** parameter vector; peacock holds no server-side UI state.
- **Parity by construction (ACC-1).** The Flutter app and the HTML runtime route
  every render through the identical `render_report` call and the identical
  `updateModelContext` record; the chat/structured/embedded surfaces share the
  same render core.
- **Independently verifiable (NFR-S-5).** `/app` (standalone fetch) and
  `/app-shim` (standalone relay) both work with no MCP host, so the surface is
  testable on its own.

## Composition with Triton's MCP-Apps proxying

Triton's MCP-Apps proxying (issue #143, parts A/B/C landed — see
`doc/triton-mcp-apps-proxying-issue.md`) routes the host ⇄ peacock channel:

1. **A — pass-through `_meta.ui.resourceUri`.** peacock's `render_report` tool
   result carries `_meta.ui.resourceUri = ui://peacock/<report>`; Triton
   forwards it to the host unchanged.
2. **B — proxy `resources/read`.** The host reads `ui://peacock/<report>`;
   Triton proxies it to peacock (`X-Triton-MCP: resources/read`) and returns the
   resource HTML. That HTML is the **shim** once `mcp.rs` cuts over (today:
   `iframe.html`). The shim then loads `/app/` from peacock's origin.
3. **C — relay the verbs.** The host's in-iframe `callServerTool('render_report',
   {absolute params})` dispatches to peacock as a fresh `tools/call`;
   `updateModelContext` records are relayed to the host's model-context channel
   unmodified. The shim ↔ Flutter bridge sits **below** this: it speaks the same
   `mcp:*` `postMessage` verbs to whatever parent it has — the host directly, or
   Triton's relay — so nothing in the bridge is Triton-specific.

The bridge is host-agnostic by design: it targets `window.parent` with the
SEP-1865 verbs and never assumes Triton. peacock continues to test all four
surfaces against its own real endpoints; the Triton-proxied variant is enabled
once #143's last items land.

## Files

- `web/peacock-web/web/index.html` — runtime base-href rewrite (Problem 1).
- `web/peacock-web/build_web.sh` — build + verify (Problem 1 check).
- `web/peacock-web/lib/mcp_bridge.dart` — platform-neutral `Mcp` interface +
  conditional import (keeps `flutter test` compiling on the VM).
- `web/peacock-web/lib/mcp_bridge_web.dart` — the web transport (host bridge +
  HTTP fallback) over `dart:js_interop` / `package:web`.
- `web/peacock-web/lib/mcp_bridge_stub.dart` — inert VM stub for tests.
- `web/peacock-web/lib/main.dart` — renders through `Mcp`; commits drills via
  `updateModelContext`.
- `crates/peacock-server/assets/flutter-shim.html` — the `ui://` runtime shim.
- `crates/peacock-server/src/http.rs` — serves the shim at `GET /app-shim`.

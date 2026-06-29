# peacock — Business Requirements & Specification

Status: draft v0.1 (2026-06-27)
Scope: **peacock**, the report renderer and MCP-App host for the open
agent-reporting architecture (escurel · peacock · triton) on plain
DuckDB. peacock is a new component: a stateless Rust service + library
that turns an escurel *report skill* plus parameters into a renderable
artifact (A2UI v0.9 + Vega-Lite + structured content + PNG), hosts the
MCP-App iframe behind Triton's MCP ingress, and supplies the chat path
through Triton.

Companion document: `HLD.md` (arc42 high-level design) in this
directory. This document states *what* peacock must do and the upfront
decisions taken; the HLD states *how* it is structured. Neither
specifies code — the coding agent has discretion on implementation
details not pinned here.

Design inputs (canonical):

- `2026-06-26-open-dives-duckdb/architecture-escurel-native.md` — the
  component architecture this spec implements (escurel = KB + data
  virtualization; peacock = renderer + iframe host; triton = chat
  adaptor). The §2–§4 boundaries are normative here.
- `2026-06-26-open-dives-duckdb/paper/paper.pdf` — the published
  framing.
- **Blueprints for structure & approach** (not modified by this spec):
  Triton (`2026-05-22-triton-gateway-spec/{requirements,architecture}.md`)
  for the gateway/service shape — single static Rust binary, central
  pivot, stateless, stdout audit, substrate deployment, `*-test-support`
  fakes; escurel (`2026-06-20-escurel-instance-backends/HLD.md`,
  `2026-05-18-kb-rust-implementation-spec/`) for the data model,
  `escurel-client` family, and the credential/ACL boundary.

## 1. Context

In the open architecture, a report is an **escurel skill** whose data
are **virtualized instances** (structured data views over external
sources, no copy), and which is consumed on three surfaces: an MCP App
inside a host (Claude/ChatGPT/VS Code), a chat channel, or as structured
data fed back to an agent. escurel deliberately does no rendering; Triton
adapts to chat protocols. **peacock is the renderer that sits between
them** — an escurel *client* that reads access-checked rows and compiles
the renderable artifact, and the host of the interactive MCP-App iframe.

peacock holds no database credentials and persists no user data. It is
the open replacement for the rendering half of MotherDuck Dives, kept
strictly separate from the knowledge base that owns the data.

### 1.1 Decisions taken upfront (design-owner answers, 2026-06-27)

These four decisions are fixed inputs to this spec; the ADRs in `HLD.md`
§9 carry them:

1. **Core language: Rust.** peacock is a single static Rust binary plus
   an embeddable Rust library crate; it embeds in Rust agents in-process.
2. **iframe runtime: Flutter web.** The MCP-App interactive surface is a
   Flutter-web bundle (A2UI v0.9 renderer + a Vega custom component),
   aligned with the escurel `escurel_explorer_kit` Flutter lineage.
3. **MCP-App exposure: behind Triton's MCP ingress.** peacock is an
   internal upstream; Triton terminates TLS/OIDC and proxies the
   MCP `tools/call` and `resources/read` traffic to peacock. peacock has
   no public ingress.
4. **v1 scope: all three surfaces** — MCP App, chat-via-Triton, and
   structured content — plus the embeddable-library face.

### 1.2 Naming

**peacock is the deployed renderer; its source is the canonical
reference** for "render an escurel report skill". There is no separate
"renderer pattern" component. Where this spec says *the renderer*, it
means peacock.

## 2. Glossary

- **Report skill** — an escurel skill (`type: skill`, `render: a2ui`)
  whose front matter declares render parameters and binds data via typed
  references to structured data views, and whose body holds the
  declarative view layout + narrative.
- **Structured data view** — a virtualized `sql_view` escurel instance:
  a read-only, ACL'd, parameterizable projection over an external source,
  referenced by `[[skill::id]]`. (escurel-owned; see escurel HLD §5.)
- **Artifact** — peacock's output for one `(report, params)`: an
  **A2UI v0.9** document (layout + a Vega custom component per chart),
  **structuredContent** (typed rows + parameter schema), and on demand a
  **PNG/SVG** rasterization.
- **Render core** — the stateless function `(report skill, params, rows)
  → artifact`. The single path every surface funnels through.
- **MCP-App surface** — the `ui://` resource (Flutter-web iframe) plus
  the tool result that links it, served behind Triton's MCP ingress.
- **Chat surface** — A2UI + PNG handed to Triton's surface mapper for
  projection onto a messenger.
- **Embedded face** — the render core exposed as a Rust library called
  in-process by an agent (e.g. to preview a report it is authoring).
- **escurel-client** — the typed escurel client crate peacock depends on.
- **Principal** — `{sub, scopes, tenant, …}`, forwarded to escurel so its
  fail-closed ACL applies; identical shape to Triton's `Principal`.
- **Substrate** — the Hetzner agent substrate v2 (Nomad/Consul/Vault +
  Tailscale + Fabio + Packer golden image), the deployment target shared
  with Triton and escurel.

## 3. Goals / Non-goals

### 3.1 Goals

1. A **single stateless render core** that compiles `(report skill,
   params, rows)` into one artifact (A2UI v0.9 + Vega-Lite charts +
   structuredContent), shared verbatim across all surfaces.
2. **escurel as the only data path**: peacock resolves the report skill
   and reads ACL-applied rows through `escurel-client`; it never holds a
   database credential and executes no source SQL itself.
3. **MCP-App hosting** behind Triton's MCP ingress: peacock serves the
   `ui://` Flutter-web iframe, returns `structuredContent` + the linked
   UI resource, and handles in-iframe interactions as drill re-renders.
4. **Chat path** through Triton: peacock returns A2UI and rasterizes
   charts to PNG (it *is* the `render_a2ui_to_png` rasterizer Triton's
   chat surface delegates to).
5. **Structured/agent surface**: the same call returns typed
   `structuredContent` an agent reads and re-drills by changing params.
6. **Embeddable Rust library**: the render core callable in-process by a
   Rust agent (authoring preview, inline default views).
7. **Vega-Lite as the canonical chart payload**, rasterized by the
   embedded `vl-convert` (Rust, no Node, no network) and rendered live
   in the Flutter iframe via a Vega custom component.
8. **Stateless, single static binary** on the substrate: no persistence,
   clean cold start, SIGTERM drain, stdout audit shipped by the substrate
   — mirroring Triton.

### 3.2 Non-goals

- **No data execution / no credentials.** peacock never attaches a
  source, runs source SQL, or holds a DSN. All data comes from escurel as
  rows. (escurel owns virtualization, credentials, ACL.)
- **No rendering logic in escurel; no chat-protocol logic in peacock.**
  Surface-specific protocol projection is Triton's; data is escurel's.
- **No public ingress.** peacock is reached only via Triton (MCP + chat)
  or embedded in-process. TLS termination and OIDC are Triton's / the
  substrate's.
- **No persistence / no server-side UI session.** Drill state lives in
  parameters / signed tokens; every render is reproducible from
  `(report, params)`.
- **No bespoke imperative UI** (cf. Dives' React). The artifact is
  declarative (A2UI + Vega-Lite JSON).
- **No LLM calls inside peacock.** Authoring (writing the report skill)
  is the agent's job; peacock only renders.
- **No multi-region HA.** Single substrate region, as with Triton.

## 4. Stakeholders

| Role | Concern |
|---|---|
| Report author (agent) | Write a report skill; preview it via the embedded library or a render call; iterate. No knowledge of channel specifics. |
| Report consumer (human) | Open a report in an MCP host or a chat channel; see default views; drill by tapping/asking; consistent result across surfaces. |
| Report consumer (agent) | Receive typed `structuredContent`; reason over fields; re-drill by changing parameters. |
| Substrate operator | peacock boots clean on a fresh alloc; SIGTERM drain works; audit lines parse; `/version` matches the image; no public port. |
| Triton operator | Register peacock as the `render_report` upstream + the MCP-App resource owner; chat surface uses peacock for A2UI + PNG. |
| escurel operator | peacock is a well-behaved read client (resolve/expand/`query_instance`); never attempts writes or credential access. |
| Security reviewer | Trust boundary: peacock holds no DB credentials, only sees ACL-released rows; render guardrails prevent fetch/compute beyond the rows. |

## 5. Functional requirements

RFC-2119 keywords. The coding agent has discretion on details not pinned.

### 5.1 Render core (FR-R)

- **FR-R-1** peacock MUST expose one render core `(report skill, params)
  → artifact` through which **every** surface (MCP App, chat, structured,
  embedded) passes. No surface may compose an artifact by another path.
  *(Single-pivot analogue of Triton's dispatcher.)*
- **FR-R-2** The render core MUST be **stateless and pure** with respect
  to `(report skill, params, rows)`: the same inputs MUST yield the same
  artifact (parsed-structure equality; JSON key order not required).
- **FR-R-3** The artifact MUST contain three coupled outputs from one
  pass: an **A2UI v0.9** layout document, the **Vega-Lite** chart specs
  it embeds (FR-V), and **structuredContent** (typed rows + the report's
  parameter schema **and the current resolved parameter values** — the
  view state, FR-X-1). Carrying the current params on the tool result is
  what lets a consuming agent know the visualization's state without a
  separate channel.
- **FR-R-4** The render core MUST validate `params` against the report
  skill's declared parameter schema before reading data; validation
  failure MUST surface as a typed `Validation` error.
- **FR-R-5** peacock MUST surface typed errors distinguishing at least
  `Auth`, `Validation`, `Data` (escurel read failure), and `Render`
  (composition/guardrail failure), each mapped per surface (MCP JSON-RPC
  code, chat error, library `Result`). *(Mirrors Triton's four-variant
  error model.)*

### 5.2 Data access via escurel (FR-D)

- **FR-D-1** peacock MUST resolve the report skill via `escurel-client`
  (`resolve`/`expand`) and MUST read each referenced structured data view
  through escurel's parameterized read `query_instance(ref, params) →
  rows`. Parameters MUST travel as typed values and MUST be executed as
  **prepared-statement (bound) parameters** — the view's `{{param}}`
  placeholders are query parameters, never text spliced into SQL. This is
  the sole sanctioned data path; there is no SQL-string path anywhere in
  peacock. *(Realises FR-D-6 / NFR-S-6; cross-spec dependency on escurel,
  §10.)*
- **FR-D-2** peacock MUST forward the caller `Principal`/tenant to escurel
  on every read so escurel's fail-closed ACL applies. peacock MUST NOT
  cache rows across principals.
- **FR-D-3** peacock MUST NOT hold any database credential, connection,
  or DSN, and MUST NOT execute source SQL. All data is rows from escurel.
- **FR-D-4** peacock MUST push aggregation to the structured data view
  (the report skill's references carry the aggregation); it MUST treat
  returned rows as already-aggregated and inject them as inline data
  (FR-V-3). It MUST NOT perform large client-side aggregation as a
  substitute for a missing view.
- **FR-D-5** On an escurel read error (timeout, ACL denial, drift), peacock
  MUST surface a typed `Data` error and MUST NOT emit a partial artifact
  that silently omits a failed view.
- **FR-D-6 (no SQL construction; bound parameters)** peacock MUST pass
  report parameters to escurel as **typed, structured values** on the
  `query_instance(ref, params)` call. peacock MUST NOT build, template,
  concatenate, or otherwise emit SQL text, and MUST NOT substitute a
  parameter into any SQL string. The `{{param}}` placeholders in a
  structured data view's `filter` denote **bound query parameters**, not
  textual substitution; binding them as prepared-statement parameters is
  escurel's obligation (§10). peacock MUST reject any parameter whose type
  does not match the report skill's declared scalar type before the call
  (defense in depth with FR-R-4). *(See NFR-S-6.)*

### 5.3 Visualization (FR-V)

- **FR-V-1** Charts MUST be specified in **Vega-Lite** and carried as a
  **custom A2UI v0.9 catalog component** (`kind: vega`). A2UI layout
  (KPI/table/text/controls) and Vega-Lite graphics MUST stay separable.
- **FR-V-2** peacock MUST rasterize a chart to PNG/SVG via the embedded
  **vl-convert** (Rust; no Node, no network) and MUST expose this as the
  `render_a2ui_to_png` capability Triton's chat surface delegates to.
- **FR-V-3** For inline rendering, peacock MUST inject the escurel rows
  into the Vega-Lite spec as inline data; the spec MUST NOT reference a
  remote data URL.
- **FR-V-4 (guardrail)** peacock MUST reject or strip any chart spec that
  (a) loads remote/external data, or (b) uses Vega expression features
  outside a documented safe subset, so an agent-authored spec cannot
  fetch or compute beyond its rows. Violations surface as `Render`
  errors. *(Security; see NFR-S-3.)*
- **FR-V-5 (optional, deferred)** peacock MAY accept a chart authored as a
  **ggsql** grammar-of-graphics snippet; when so, compilation to Vega-Lite
  MUST happen where the DuckDB connection lives (escurel-side), and peacock
  MUST receive a Vega-Lite spec. ggsql MUST NOT be a load-bearing
  dependency in v1 (gated on it maturing past alpha). *(See §7, §9.)*

### 5.4 MCP-App surface (FR-M)

- **FR-M-1** peacock MUST serve the report's interactive UI as a
  **`ui://` MCP-Apps resource** whose content is the **Flutter-web**
  bundle that renders the A2UI document live (with the Vega component via
  vega-embed). *(MCP-Apps; Flutter-web per §1.1.)*
- **FR-M-2** A `render_report` tool result MUST carry `structuredContent`
  **and** link the UI resource (per the MCP-Apps `_meta.ui.resourceUri`
  convention) so the host renders the iframe.
- **FR-M-3** peacock MUST handle a committed in-iframe drill as **two
  coupled actions**: (a) `callServerTool('render_report', …)` for a fresh
  `(report, new params)` re-render of the iframe, and (b) a push of the
  updated **view state** into the conversational model (FR-X-3). No
  server-side UI/session state is kept (FR-R-2).
- **FR-M-4** The MCP-App surface MUST be reachable **only behind Triton's
  MCP ingress**: Triton terminates TLS/OIDC, mints a Vault-scoped upstream
  token, and proxies `tools/call` and `resources/read` to peacock. peacock
  MUST accept the Triton-forwarded principal and MUST NOT expose a public
  MCP endpoint. *(§1.1 decision 3; cross-spec dependency on Triton, §10.)*

### 5.5 Chat surface (FR-C)

- **FR-C-1** For the chat path, peacock MUST return a pre-shaped A2UI v0.9
  document that Triton's surface mapper passes through and projects per
  channel (Triton FR-U-5 / FR-A-9).
- **FR-C-2** peacock MUST serve chart rasterization to PNG for chat
  (FR-V-2); Triton's chat surface delegates dashboard rendering to it
  (Triton FR-A-11).
- **FR-C-3** Drill-downs initiated from chat arrive as Triton-verified
  `(tool, args)` (HMAC-signed token, Triton FR-A-12) and MUST enter the
  render core as a fresh `(report, params)` render (FR-M-3 parity).

### 5.6 State synchronization — visualization ↔ conversation (FR-X)

The interactive surfaces must keep the visualization's state and the
conversational agent's context in sync, so a drill the user makes is
available to the agent for follow-ups (including spawning other
visualizations).

- **FR-X-1 (state = params)** The complete view state of a report MUST be
  its `(report_id, params)` — there is no hidden server-side visualization
  state (FR-R-2). The current resolved parameter values MUST be returned in
  `structuredContent` (FR-R-3) so the state rides every tool result.
- **FR-X-2 (conversation is authoritative; absolute params)** Every
  interaction that changes state MUST carry the **absolute** parameter
  vector (never a delta) and MUST be applied as a fresh render. The
  conversational host/agent holds the authoritative running state; peacock
  holds none. This keeps renders idempotent and the iframe and context from
  drifting.
- **FR-X-3 (push committed state to the model)** On a **committed** drill on
  the MCP-App surface, the iframe runtime MUST publish the updated view
  state to the model via the MCP-Apps **`updateModelContext`** channel, as a
  **compact** record `{report_id, params, salient_summary}` — NOT the row
  data. **Ephemeral** interactions (hover, zoom, transient highlight) MUST
  NOT be promoted. For A2UI hosts the equivalent client-to-server data-sync
  / event MUST be used; where neither channel exists, the tool-call record
  plus `structuredContent` (FR-R-3) are the fallback.
- **FR-X-4 (bidirectional)** An agent-initiated `render_report` MUST update
  the same view state and re-render the surface, so visualization and
  context stay consistent whether the human or the agent drives the change.
- **FR-X-5 (chat path)** On the Triton chat path the signed `(tool, args)`
  drill token carries the absolute params (FR-C-3); when a conversational
  agent mediates the channel, that tool call is the context-sync. Pure
  report-bot chat with no agent in the loop has no model context to sync.
- **FR-X-6 (shared selection — see §9 OQ-5)** Whether a committed selection
  is promoted to a **shared exploration selection** reusable across other
  visualizations (vs staying per-report params) is an open decision; the
  recommended default for multi-visualization follow-ups is a shared,
  named selection in the conversation context that new renders inherit.

### 5.7 Embedded library (FR-E)

- **FR-E-1** peacock MUST expose the render core as a Rust library so a
  Rust agent can produce an artifact in-process (authoring preview, inline
  default views) using the same code path as the service (FR-R-1).
- **FR-E-2** The embedded face MUST produce artifacts (A2UI / Vega-Lite /
  structuredContent / PNG); it does **not** host the iframe (the iframe is
  a service-face concern, FR-M-1).
- **FR-E-3** The embedded face MUST require the caller to supply an
  escurel binding + principal; it MUST NOT acquire database credentials.

### 5.8 Identity (FR-I)

- **FR-I-1** peacock MUST accept the principal forwarded by Triton (the
  Vault-minted, OIDC-derived upstream token), in the same `Principal`
  shape Triton uses, and forward it to escurel (FR-D-2).
- **FR-I-2** peacock MUST NOT perform inbound OIDC verification itself for
  the service face (Triton does, at the boundary). A dev-token fallback
  MAY exist but MUST be gated behind a build-time `cfg`, rejected in
  production builds. *(Mirrors Triton FR-I-5 / ADR-10.)*

### 5.9 Observability (FR-O)

- **FR-O-1** peacock MUST expose `GET /healthz` returning ok once ready.
- **FR-O-2** peacock MUST expose `GET /version` returning binary + image
  SHAs (and the embedded Flutter-bundle SHA).
- **FR-O-3** peacock MUST expose a tailnet-only metrics endpoint (render
  latency, escurel read latency, rasterization latency, guardrail
  rejections), not on any public path.
- **FR-O-4** peacock MUST emit one structured audit line per render to
  stdout (`who`/tenant, report id, params hash, surface, result,
  latency_ms, trace_id), and MUST NOT ship logs itself (substrate
  collector tails stdout). Rows, tokens, and PII MUST NEVER appear in
  logs. *(Mirrors Triton FR-AU.)*

### 5.10 Lifecycle (FR-L)

- **FR-L-1** peacock MUST cold-start with no local state on a fresh Nomad
  allocation and pass `/healthz`.
- **FR-L-2** On SIGTERM/SIGINT, peacock MUST stop accepting new work,
  drain in-flight renders to a per-request deadline, flush stdout, exit 0.
- **FR-L-3** At cold start peacock MUST load its manifest (component
  catalog, render policy/guardrails, escurel binding) and closed-check
  every enumerated value; boot MUST refuse on any unknown value. Every
  secret field MUST be a Vault reference in production. *(Mirrors Triton
  FR-L-4/FR-L-6, ADR-13.)*

## 6. Non-functional requirements

### 6.1 Security (NFR-S)

- **NFR-S-1** No static cloud/database credentials in the binary, image,
  or job env. peacock holds no DSN; escurel owns credentials. *(Trust
  boundary.)*
- **NFR-S-2** No public surface: peacock ports are tailnet-only, reached
  via Triton; not Fabio-`urlprefix`-tagged. *(§1.1 decision 3.)*
- **NFR-S-3** Render-guardrail invariant: a rendered artifact MUST NOT be
  able to fetch data or execute arbitrary computation beyond the rows
  escurel released — enforced by FR-V-4 (inline-only data, restricted
  Vega expr) and the Flutter iframe's sandbox.
- **NFR-S-4** peacock processes attacker-influenced input (agent-authored
  report skills, params) and has a network path to escurel; it MUST NOT
  hold ambient credentials, and the forwarded principal token MUST be
  short-lived (Vault-minted by Triton, TTL ≤ 5 min). *(Lethal-trifecta
  cut, mirroring Triton NFR-S-3.)*
- **NFR-S-5** Air-gap stance: the only egress is to escurel (tailnet),
  Vault/Consul (substrate), and stdout. No public-internet egress; in
  particular the rasterizer (vl-convert) is self-contained and MUST NOT
  fetch from the network.
- **NFR-S-6 (SQL-injection / parameter binding)** Report parameters
  originate from agents and end users and are untrusted. They MUST be
  carried as **typed values end-to-end** and bound as **prepared-statement
  parameters** at the point of execution; **no component may build a SQL
  string by interpolating a parameter.** Specifically: peacock constructs
  no SQL and forwards typed params only (FR-D-6); escurel's `query_instance`
  MUST bind the structured data view's `{{param}}` placeholders as query
  parameters, never by textual substitution (§10). For the optional ggsql
  path, where a fresh connection precludes session-bound parameters
  (FR-V-5), parameters MUST be injected as **type-checked literals via a
  safe allowlist** (typed scalars only — date/number/enum), never raw
  concatenation, and identifier-position parameters (column/table names)
  MUST be validated against a fixed allowlist. This invariant is the
  primary mitigation alongside escurel's fail-closed ACL (FR-D-2).

### 6.2 Performance (NFR-P)

- **NFR-P-1** Render-core overhead beyond the escurel read SHOULD be
  modest (target: composition + rasterization not dominating a typical
  report render).
- **NFR-P-2** A drill re-render SHOULD reuse warm escurel connections and
  cached report-skill resolution where the principal/tenant is unchanged
  within a session window.
- **NFR-P-3** Rasterization MUST be bounded; oversized result sets MUST be
  rejected with a typed `Render` error rather than rendered unboundedly.

### 6.3 Operability (NFR-O)

- **NFR-O-1** Config via CLI flags + `PEACOCK_*` env + a lean manifest;
  Nomad-template friendly; Vault refs mandatory for secrets in production.
- **NFR-O-2** Single static Rust binary; **no Node or Python runtime in
  the allocation**. The Flutter-web bundle is built in CI and embedded as
  static assets in the binary / served from the alloc. *(Mirrors Triton
  NFR-O-2; build-time-only frontend toolchain.)*
- **NFR-O-3** Light resource budget (no model resident, no DB engine
  resident); default substrate client class sufficient.

### 6.4 Portability / licensing (NFR-PT)

- **NFR-PT-1** Build target `linux/x86_64` (release-blocking);
  `linux/aarch64` + `macos/arm64` best-effort for local dev.
- **NFR-PT-2** Third-party crates statically linked; only dynamic
  dependency is libc.
- **NFR-PT-3** License: **BSL-1.1 → MPL-2.0** after the standard term,
  matching escurel. *(Confirm with the design owner if a different license
  is desired — see §9 OQ-4.)*

## 7. Out of scope / deferred

- **ggsql as a hard dependency** — optional only (FR-V-5); the Vega-Lite
  path is canonical. Revisit when ggsql is past alpha and the inline-data
  question (§9 OQ-1) is settled.
- **Mosaic/vgplot live big-data cross-filter** — the inline-data +
  re-render-on-drill model is the default; million-row in-browser
  cross-filter is a later option (a second client runtime).
- **Saved/shared report instances** — persisting a parameterized render as
  an escurel instance (bookmark/share) is an escurel concern; peacock
  renders transiently in v1.
- **Authoring tooling** (the `dive` meta-skill / `get_report_guide`) —
  belongs to the escurel/authoring-agent side, not peacock.
- **Public/standalone MCP endpoint** — deferred; v1 is behind Triton.
- **Multi-region HA.**

## 8. Acceptance criteria

- **ACC-1 Single render path.** MCP-App, chat, structured, and embedded
  surfaces produce parsed-structure-identical A2UI + structuredContent for
  the same `(report, params, principal)`. *(FR-R-1, FR-R-2.)*
- **ACC-2 Credential-free data path.** With escurel returning ACL-applied
  rows, peacock renders without holding any DSN/credential; a static check
  confirms no DB driver/credential surface in peacock. *(FR-D-2/3.)*
- **ACC-3 ACL pass-through.** A principal lacking access to a referenced
  view yields a typed `Data`/`Auth` error, not a partial render. *(FR-D-5,
  FR-I-1.)*
- **ACC-4 Guardrail.** A report skill whose chart spec references a remote
  data URL or disallowed expr is rejected with a `Render` error.
  *(FR-V-4.)*
- **ACC-5 MCP-App via Triton.** Through Triton's MCP ingress, a
  `render_report` call returns structuredContent + a linked `ui://`
  resource; `resources/read` returns the Flutter bundle; an in-iframe
  drill produces a fresh render. *(FR-M-1..4.)*
- **ACC-6 Chat via Triton.** Triton's chat surface renders a report by
  calling peacock for A2UI + a PNG; a signed drill re-renders. *(FR-C-1..3.)*
- **ACC-7 Embedded.** A Rust test embeds the library, supplies an escurel
  fake + principal, and gets an artifact via the same path as the service.
  *(FR-E-1..3.)*
- **ACC-8 Stateless re-render.** Two identical `(report, params)` calls,
  and a drill followed by reverting the param, reproduce byte-comparable
  artifacts; no server-side session state exists. *(FR-R-2, FR-M-3.)*
- **ACC-9 Cold start / drain.** Fresh alloc binds, passes `/healthz`;
  SIGTERM drains in-flight renders and exits 0. *(FR-L-1/2.)*
- **ACC-10 Manifest closed-set.** An unknown manifest enum or a literal
  secret in production mode refuses boot with a named error. *(FR-L-3.)*
- **ACC-11 Injection-safe parameters.** A drill/param value containing SQL
  metacharacters (e.g. `'; DROP TABLE …`, `1 OR 1=1`, a UNION fragment)
  changes only the bound value: escurel runs it as a prepared-statement
  parameter (returning empty/normal results, never executing it), peacock
  emits no SQL string, and a non-conforming type is rejected before the
  call. A static check confirms peacock has no SQL-string construction
  path. *(FR-D-6, NFR-S-6.)*
- **ACC-12 State sync.** A committed in-iframe drill (a) re-renders the
  iframe and (b) pushes a compact `{report_id, params, summary}` view-state
  record to the model via `updateModelContext`; the current params appear in
  the tool result's `structuredContent`; an ephemeral hover/zoom does not
  promote state; and a subsequent agent-initiated render reflects the
  drilled selection. *(FR-X-1..4, FR-R-3.)*

## 9. Decisions log & open questions

**Resolved (design owner, 2026-06-27):** D-1 Rust core + library; D-2
Flutter-web iframe; D-3 MCP-App behind Triton MCP ingress; D-4 all three
surfaces in v1. (See ADRs in `HLD.md` §9.)

**Open questions** — flagged, not assumed:

- **OQ-1 (ggsql `spec` data inlining).** Does ggsql `spec` mode inline the
  queried data into the Vega-Lite JSON (required for headless vl-convert
  and FR-V-3), or reference its HTTP server? Settle before FR-V-5 leaves
  "optional". *(Empirically checkable.)*
- **OQ-2 (escurel `query_instance` shape).** Is the parameterized
  result-set read a new escurel MCP tool or a promoted `run_stored_query`?
  Affects the escurel cross-spec dependency (§10).
- **OQ-3 (structuredContent transport on the chat path).** For chat, is
  structuredContent surfaced to a co-present agent, or only the A2UI/PNG?
  Default assumption: A2UI/PNG to humans, structuredContent on the MCP /
  embedded paths — **confirm**.
- **OQ-4 (license).** BSL-1.1 → MPL-2.0 assumed to match escurel —
  confirm.
- **OQ-5 (shared exploration selection vs per-report params).** Should a
  committed drill be promoted to a **shared, named selection** in the
  conversation context that *other* visualizations inherit (good for
  "now show me X for this"), or stay scoped to one report's params? Default
  per-report; shared-selection recommended for multi-visualization
  follow-ups (FR-X-6). *(Affects the view-state schema and the agent-side
  contract.)*

## 10. Cross-spec dependencies

peacock relies on focused, in-character extensions to the two blueprint
components. These are obligations of those specs, recorded here:

- **escurel** — a parameterized, ACL-checked result-set read over
  structured data views, `query_instance(ref, params) → rows` (today the
  SQL-view backend returns only bounded scalar projections). The read
  MUST bind the view's `{{param}}` placeholders as **prepared-statement
  parameters** (never textual substitution); value-position params bind
  directly, and any identifier-position param MUST be allowlist-validated.
  *(escurel HLD §3.3/§5; NFR-S-6; OQ-2.)*
- **triton** — MCP-Apps proxying in the MCP adapter: relay a
  `render_report` upstream result carrying `structuredContent` +
  `_meta.ui.resourceUri`, proxy `resources/read` to the resource-owning
  upstream (peacock), and relay `callServerTool` drills. The chat surface
  registers peacock as the `render_report` upstream and as the
  `render_a2ui_to_png` rasterizer. *(Triton FR-A-6/FR-A-11/FR-U-5.)*

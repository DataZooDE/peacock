---
title: "Peacock: an open architecture for agent-authored and agent-consumed data reports over a virtualized knowledge base"
author:
  - Joachim Rosskopf
date: 27 June 2026
abstract: |
  Cloud data warehouses now ship agent-driven reporting tools — MotherDuck
  *Dives* being the clearest example — in which a large-language-model agent
  writes an interactive dashboard directly on top of live data. These tools
  are powerful but couple the report to a single proprietary backend and to
  one consumption surface, and they do not keep an interactive drill in step
  with the agent's own conversational context. We describe an open
  architecture that delivers the same capability on plain DuckDB, with
  reports that are both *authored by* an agent and *consumed by* an agent,
  and that surface identically inside an MCP host, inside a chat messenger,
  or as structured data fed back into an agent's context. Three components
  separate the concerns: **escurel**, a knowledge base whose *data
  virtualization* turns external relations into typed, access-controlled
  instances; **peacock**, a new stateless renderer and iframe host that
  compiles a report into a declarative A2UI/Vega-Lite artifact; and
  **triton**, a gateway that adapts that artifact to chat protocols. We
  develop the design on a single running example — monthly revenue by
  category over the Northwind dataset — and give particular attention to two
  mechanisms: how a *structured data view* is declared, virtualized without
  copying data, referenced by a typed link, and read through a
  parameterized, access-controlled query; and how a visualization's state —
  which is exactly its parameter vector — is kept synchronized with the
  conversation, so that a drill a human performs becomes context the agent
  can build on. Pushing rendering out of the knowledge base into peacock,
  while keeping data and credentials in escurel and the running state in the
  conversation, yields a clean trust boundary and one artifact that serves
  humans and agents alike.
---

# 1. Introduction

A recurring pattern in 2025–2026 data tooling is the *agent-authored
dashboard*: a user describes an analysis in natural language, an agent
explores the schema, writes the queries and the visualization, and the
result is saved as a living report that re-runs against current data.
MotherDuck's *Dives* is the most explicit instance — a Dive is a React
component (`dive.tsx`) that executes SQL through a hook against the
MotherDuck backend and renders charts, authored by an external agent over
the MotherDuck MCP server and persisted in the cloud workspace[^dives].
The artifact is, in effect, a dashboard-as-code; notably, the language
model lives in the *authoring agent*, not in SQL, so the runtime has no
hidden in-database AI dependency[^dives].

This design is attractive but carries three gaps. First, it is bound to a
proprietary cloud engine (MotherDuck's hybrid "dual execution") for the
live query path. Second, the report is essentially *one* artifact for *one*
surface — a browser view — and is not natively consumable by another agent
as structured data, nor portable to a chat channel or a different host.
Third, the interactive dashboard and the agent's conversation drift apart:
when a human drills into the chart, the agent that could answer the next
question — or build a related view — does not know it happened.

We set out to reproduce the capability on **plain DuckDB**, under
requirements the proprietary design does not fully meet: a report must be
(i) *created by an agent*, and (ii) *consumed by an agent* — through default
views and conversational drill-downs whose state stays **synchronized with
the agent's context** — surfaced either inside an MCP host, through chat
messages, or as machine-readable structured output. DuckDB's new
client–server *Quack* protocol[^quack] removes the cloud-engine coupling by
allowing concurrent, network-accessible (and even in-browser) DuckDB
clients; the rest is an architecture question.

Our answer is a three-component system built from two existing DataZooDE
projects plus one new one. The contribution of this paper is the
architecture and, specifically, the introduction of **peacock**, the
component that renders reports, hosts their interactive surface, and keeps
that surface's state in step with the conversation — kept deliberately
separate from the knowledge base that holds the data. Throughout we develop
the design on **one running example** — *monthly revenue by product
category* over the classic **Northwind** trading dataset[^northwind] —
building each component against it rather than in the abstract.

# 2. Architecture

## 2.1 Three concerns, three components, two delivery paths

The system factors the problem into three single-responsibility components
(Fig. 1):

- **escurel** — a multi-tenant knowledge base over DuckDB. It owns the
  *report definitions* (as skills), the *data* (as virtualized instances),
  the *credentials and access control*, and a parameterized read path. It
  performs no rendering[^escurel].
- **peacock** *(new)* — a stateless renderer and iframe host. It is an
  escurel *client*: given a report and parameters it reads rows from escurel
  and compiles them into a declarative A2UI/Vega-Lite artifact, rasterizes
  charts to PNG, hosts the interactive MCP-App surface, and keeps that
  surface's state synchronized with the conversation. It holds no database
  credentials and persists nothing.
- **triton** — a multi-protocol gateway that acts as the *adaptor to the
  chat protocols*, projecting peacock's artifact onto Telegram, Microsoft
  Teams, Discord and similar channels, and routing signed interaction tokens
  back as drill-downs[^triton].

A single peacock artifact reaches consumers along two paths. On the
**MCP-App path** (`escurel → peacock`), peacock acts as an MCP server: it
returns the report as structured content plus a `ui://` resource whose
sandboxed iframe renders live, interactive Vega-Lite charts inside an MCP
host such as Claude or Microsoft Copilot Studio[^mcpapps]. On the **chat
path** (`escurel → peacock → triton`), peacock hands triton a pre-shaped
A2UI document and PNG renderings, which triton maps to the conventions of
each messenger. The same `render_report` call also returns typed
`structuredContent`, which is what a *consuming agent* reads — it reasons
over fields and re-drills by changing parameters, never by scraping a
rendered view. A theme that runs through what follows: because peacock is
stateless and a render is fully determined by `(report, params)`, the
**conversation holds the authoritative running state** and peacock is simply
re-invoked — which is also what makes state transfer (§2.4) clean.

<figure>
<img src="figures/architecture.svg" alt="Three-component architecture"/>
<figcaption><strong>Figure 1. The three components and their interplay.</strong>
escurel virtualizes external sources into typed, access-controlled
instances and answers a parameterized row query. peacock (new) reads those
rows and compiles a report into an A2UI/Vega-Lite artifact; it hosts the
MCP-App iframe directly, emits structured content for agents, and
synchronizes the view's state with the conversation. triton adapts the same
artifact to chat protocols. Data and credentials never leave escurel;
rendering never enters it.</figcaption>
</figure>

## 2.2 escurel: knowledge base and data virtualization

escurel organizes everything as Markdown pages under a *skill::instance*
model[^skillinst]. A **skill** is a page with `type: skill` whose front
matter declares the schema its instances must satisfy (`required_frontmatter`,
`optional_frontmatter`), an `owner_field` and an `acl` block, and —
optionally — a `backend` block binding the skill to an external data source.
An **instance** is a page declaring which skill it conforms to. The decisive
property is that the type vocabulary *is* the set of skill pages: adding a
new kind of object means writing a Markdown file, not changing code. This is
exactly why we model a report as a skill (§2.4): report types become
first-class, discoverable (`list_skills`), validated, access-controlled
objects, with no parallel schema system.

What turns escurel from a notes store into a *data* layer is its **data
virtualization**: external relations are presented as instances *without
copying the data*[^escurel]. In our running example, the Northwind sales
figures live in an operational store; escurel exposes them as a single
instance — a structured data view called `nw_revenue_by_category` — that a
report can reference, while the rows themselves never leave the source. A
skill whose `backend` is a `sql_view` declares a read-only source (a
connector such as `postgres`/`duckdb`/`parquet`, an admin-registered
*credential handle* — never a DSN in Markdown — a relation, an optional
filter, and a projection of source columns to instance fields). Creating
the instance makes escurel attach the source read-only, materialize a
deterministic `VIEW`, capture a schema fingerprint, and write a small
*overlay* Markdown page recording a `backend_ref`
(`{kind, view, binding_hash, source_schema_fingerprint}`). The overlay's
body remains human- and agent-authored prose — the *semantic layer* that
says what the view means, its caveats and owners.

Three consequences matter for reporting. **(i) One trust boundary.**
Credentials and the fail-closed ACL live only in escurel; any consumer,
including peacock, receives *rows that have already been access-checked* and
never holds a database secret. **(ii) Semantics travel with data.** Because
each virtualized relation carries an overlay page, an authoring agent can
`search`/`resolve`/`neighbours` over a metric glossary and view
documentation to ground itself before writing the Northwind report — a
capability the proprietary design lacks. **(iii) Uniform retrieval.**
Virtualized instances participate in the same hybrid vector + full-text
search as native pages, so reports and the data they cite are discoverable
through one interface.

## 2.3 Structured data views

A *structured data view* is the unit a report selects data from, and it is
precisely a virtualized `sql_view` instance. Its life cycle has four steps,
each owned by escurel, which we follow on `nw_revenue_by_category`:

1. **Declare.** The skill's `backend: sql_view` block names the source,
   credential handle, relation, optional filter, column-to-field projection,
   and the columns that feed full-text search — the view's contract:

```yaml
---
type: skill
id: nw_revenue_by_category
backend:
  kind: sql_view
  source:
    connector: duckdb              # or postgres / parquet …
    attach: northwind              # admin-registered credential handle
    relation: |
      SELECT date_trunc('month', o.order_date)                    AS month,
             c.category_name                                       AS category,
             sum(od.unit_price * od.quantity * (1 - od.discount))  AS revenue
      FROM orders o
      JOIN order_details od ON od.order_id  = o.order_id
      JOIN products      p  ON p.product_id = od.product_id
      JOIN categories    c  ON c.category_id = p.category_id
      GROUP BY 1, 2
    filter: "month BETWEEN {{from}} AND {{to}}
             AND ({{category}} = 'ALL' OR category = {{category}})"
  project:     { month: month, category: category, revenue: revenue }
  search_text: [category]
owner_field: sales_owner
acl: { read: [sales] }
---
EMEA revenue by month and Northwind category, mirrored read-only. One VIEW;
rows are virtualized, not stored. The instance overlay carries a backend_ref.
```

2. **Materialize.** On instance creation escurel attaches the source
   read-only, creates the deterministically named `VIEW`, records a binding
   hash and a source-schema fingerprint, and writes the overlay page. The
   fingerprint lets escurel detect upstream drift and lets audit rebuild the
   view from the binding alone.
3. **Reference.** A report binds the view by a *typed wikilink*,
   `[[nw_revenue_by_category]]`, validated at index time (skill exists,
   target exists, target's `skill` matches). Data selection is a reference,
   not an embedded SQL string — the report never contains credentials or raw
   queries.
4. **Query.** A report needs full, *parameterized* result sets for charts,
   beyond the bounded scalar projections escurel exposes today for previews.
   The view is read through one parameterized, access-controlled call —
   `query_instance(ref, params) → rows` — where the report's parameters (a
   date range, a category drill) bind into the view's filter. The `{{from}}`
   / `{{category}}` placeholders are **bound query parameters** (prepared-
   statement parameters, not text substitution): an untrusted value can
   change only what the filter compares against, never the query's
   structure, so the parameter path is injection-safe. Aggregation is pushed
   into the view, so DuckDB performs the `GROUP BY` and only tidy,
   already-aggregated rows cross the boundary. Read-only enforcement,
   fail-closed ACL and view-grain access are unchanged from the existing
   instance-backend design[^escurel]; the one addition is this
   parameterized result-set read.

A report is thus a composition of typed references to structured data
views; rendering it is: resolve the report skill, call `query_instance` for
each referenced view with the current parameters, and hand the rows to
peacock. Because the view is typed, access-controlled, parameterizable and
semantically annotated, the report inherits all four properties for free —
and, as §2.4 shows, a drill is just the same call with different parameters.

## 2.4 peacock: rendering, iframe hosting, and state transfer

**peacock** is the new component and the only genuinely new codebase. It is
the renderer the knowledge base deliberately omits: in escurel's own design,
UI rendering is a separate *consumer* of its read-only surface[^escurel].

A report's UI specification is itself an escurel skill. For our example, the
`northwind-monthly-revenue` skill declares the render parameters, binds data
by reference to the structured data view, and lays out the views; its body
is the agent-authored narrative:

```yaml
---
type: skill
id: northwind-monthly-revenue
render: a2ui
description: Northwind monthly revenue by product category (EMEA).
params:
  from:     { type: date,   default: "1997-01-01" }
  to:       { type: date,   default: "1997-12-31" }
  category: { type: string, default: "ALL" }      # set by a drill
data:
  rev_by_cat: "[[nw_revenue_by_category]]"         # → the structured data view
views:
  - { kind: kpi,   data: rev_by_cat, agg: "sum(revenue)", label: "Total revenue" }
  - { kind: vega,  data: rev_by_cat, spec: rev_line }    # chart (spec below)
  - { kind: table, data: rev_by_cat }
drills:
  - { on: "vega.legend", sets: { category: "$series" }, tool: render_report }
acl: { read: [sales], update: [analyst] }
---
Revenue is recognised at order date, net of line discount. EMEA orders only.
```

`render_report(skill, params)` resolves this skill, reads rows via
`query_instance` (§2.3), and composes three coupled outputs in one pass:
an **A2UI v0.9** layout document (KPI tile, chart, table)[^a2ui]; the chart
as a **custom A2UI catalog component** carrying a **Vega-Lite** spec with the
queried rows injected inline; and **structuredContent** (the typed rows, the
parameter *schema*, and the *current* parameter values). We choose Vega-Lite
deliberately (Dives uses imperative React/Recharts): it is declarative JSON,
hence agent-emittable and safe to generate, and has a self-contained Rust
rasterizer, `vl-convert`, that produces PNG with no Node.js or network
dependency[^vega]. The chart for our example is one short spec:

```json
{
  "mark": "line",
  "encoding": {
    "x":     {"field": "month",   "type": "temporal",     "title": "Month"},
    "y":     {"field": "revenue", "type": "quantitative", "aggregate": "sum"},
    "color": {"field": "category","type": "nominal"}
  }
}
```

peacock renders this at two fidelities from the *same* document: the chat
surface gets a `vl-convert` PNG; the MCP-App iframe runs `vega-embed` for a
live, interactive chart. The end-to-end path for a default view is:

```
consumer → render_report(northwind-monthly-revenue, {from, to, category})
  → escurel.resolve(skill)
  → escurel.query_instance([[nw_revenue_by_category]], params)   # ACL-applied rows
  → peacock: A2UI { kpi, vega(rev_line, rows inline), table }
             + structuredContent (rows + param schema + current params)
             + PNG (vl-convert) for chat
  → surface renders
```

**State transfer.** This is where the third gap from §1 is closed, and it
falls out of statelessness rather than being bolted on. Because a render is
fully determined by `(report, params)`, the visualization's *entire state is
its parameter vector* — there is no hidden view state, and the
conversation/host holds it authoritatively. A **committed** drill therefore
does two things at once. Suppose the user clicks *Beverages* in the legend:
peacock (a) re-renders via `callServerTool('render_report', {…, category:
"Beverages"})` so the human sees the drilled chart, and (b) publishes the
new state to the model via the MCP-Apps `updateModelContext` channel as a
**compact** record `{report, params, summary}` — not the rows, which stay in
`structuredContent`. The agent thereby learns, without scraping pixels, that
the user is now looking at Beverages, and can answer a follow-up or spawn a
*sibling* visualization that inherits the selection. The channel is
bidirectional: an agent-initiated `render_report` ("filter to Q4") updates
the same state and re-renders the iframe. Two rules keep the two projections
from drifting: drills carry the **absolute** parameter vector, never a
delta, so renders stay idempotent; and only *committed* selections are
promoted — an ephemeral hover or zoom stays local. peacock holds none of
this running state; it is re-invoked, like any pure function, with whatever
parameters the conversation now dictates.

As a safety measure peacock restricts charts to inline data and disallows
remote data loading and arbitrary expression evaluation, so an
agent-authored specification cannot fetch or compute beyond its rows.
peacock ships two faces from one core: an **embeddable library** an agent
links in-process (to preview a report it is authoring, or render a default
view inline in a conversation), and a **standalone MCP server / service**.
It is stateless — skill plus parameters plus rows in, artifact out — and, by
construction, never holds a database credential.

An optional authoring convenience sits one level above the chart format.
Because the payload is Vega-Lite, a chart may be written not only as
Vega-Lite directly but as a compact grammar-of-graphics expression that
*compiles* to it: Posit's **ggsql** extends DuckDB SQL with
`VISUALISE … DRAW …` clauses and emits a Vega-Lite specification[^ggsql].
Since ggsql runs inside the same DuckDB escurel uses for virtualization and
targets exactly the format peacock consumes, it is additive rather than
competing. We treat it as opt-in: Vega-Lite remains the canonical payload,
while a chart may equivalently be authored in ggsql and compiled where the
DuckDB connection already lives. Two caveats temper near-term reliance —
ggsql is at an early (alpha) stage, and because its queries run on a fresh
connection it cannot see session state, so parameterized drill-downs there
must inject type-checked literals via an allowlist rather than bind session
parameters.

## 2.5 triton, and the same report on two surfaces

For chat delivery, **triton** is the gateway. It already provides the seam:
its upstream router passes a pre-shaped A2UI document through unchanged, its
surface mapper projects A2UI onto each messenger's native controls (inline
keyboards, Adaptive Cards, and degraded numbered prompts where a channel is
text-only), and every interactive element carries an HMAC-signed
`{tool, args}` token so a tap becomes an authenticated drill-down[^triton].
triton's responsibility narrows to exactly that: register peacock as the
upstream renderer, project its A2UI, embed its PNGs, and route signed drills
back. No MCP-App or iframe work falls on triton.

The identical `render_report` call therefore lands on two very different
consumers (Fig. 2) — same report skill, same structured data view, same
Vega-Lite spec — differing only in surface fidelity *and in how state
transfer is carried*. On **WhatsApp**, a human sees a PNG and a numbered
prompt; the signed token is the chat-path carrier of the drilled parameters,
and where a conversational agent fronts the channel that re-invocation is
how the selection enters its context — the chat analogue of
`updateModelContext` (for A2UI hosts, the corresponding v0.9 client-to-server
data-sync plays the same role[^a2ui]). On **Microsoft Copilot Studio**, an
MCP host, the agent reasons over peacock's `structuredContent` while a human
manipulates the live iframe chart, and a click pushes the state back through
`updateModelContext`. The agent-consumes / human-views duality, and the
synchronization that keeps them coherent, come from one report definition,
one data path, and one renderer.

<figure>
<img src="figures/surfaces.svg" alt="The same Northwind report on WhatsApp and Microsoft Copilot Studio"/>
<figcaption><strong>Figure 2. One artifact, two surfaces, one synchronized
state.</strong> Left — <strong>WhatsApp</strong> (chat via triton): no native
buttons, so triton posts peacock's vl-convert PNG with the table as chunked
text and the category drills as a numbered prompt; a numbered reply carries
the signed token and triggers a fresh render, and — where an agent fronts the
channel — enters its context. Right — <strong>Microsoft Copilot Studio</strong>
(an MCP host via triton's MCP ingress): the agent reasons over peacock's
<em>structuredContent</em> and renders the MCP-App <code>ui://</code> card —
the Flutter iframe with the live Vega-Lite chart; clicking a category issues
<code>callServerTool</code> (re-render) and <code>updateModelContext</code>
(state into the conversation). On a host without MCP-Apps UI the right surface
degrades to the structured answer plus the same PNG.</figcaption>
</figure>

# 3. Discussion

The architecture's central choice is *where rendering lives*. By placing it
in peacock rather than in the knowledge base, and keeping all data and
credentials in escurel, we obtain a clean trust boundary: peacock is
stateless and credential-free, and a compromised or buggy renderer cannot
exfiltrate data it was not already granted, because it only ever sees rows
escurel's fail-closed ACL has released. The same boundary makes peacock
safely *embeddable* in an arbitrary agent — the agent gains a renderer, not
database access.

A second choice is *declarative over imperative*. Where Dives emits React,
every artifact here is data: the report is a Markdown skill, the layout is
A2UI JSON, the chart is Vega-Lite JSON. This lets one artifact serve a human
iframe, a chat channel and a consuming agent at once, and makes report
authoring safe to delegate to an agent — declarative specifications cannot
execute arbitrary code. The cost is expressiveness: arbitrary bespoke UI is
not possible, an acceptable trade for portability and agent-legibility, with
an MCP-App-only HTML escape hatch if a report ever needs it.

A third choice — and the one that closes the human/agent split — is to make
**state the parameter vector and the conversation its authoritative
holder.** Because the renderer is a pure function of `(report, params)`,
there is no second, hidden store of view state to reconcile; "synchronizing
the visualization with the conversation" reduces to keeping one value
visible to both, which a committed drill does by re-rendering *and* pushing a
compact record into the model's context. The same move generalizes the
Beverages drill beyond one chart: a committed selection can be promoted to a
small, named *exploration selection* in the conversation that sibling
visualizations inherit ("now show me orders for this") — whether to do that,
or to keep selections scoped per report, is the main open design question.
The notable property is that this synchronization required no statefulness
in the renderer; statelessness is what made it cheap.

Several caveats are honest to state. Both escurel and triton are, at the
time of writing, pre-1.0 specifications with partial implementations, and
DuckDB's Quack protocol is in beta pending v2.0; a near-term build should
keep server-side execution (the `sql_view` attach straight to DuckDB, or a
templated SQL service) as the default and treat in-browser DuckDB as a later
option. Two genuinely new capabilities must be built: escurel's
parameterized, injection-safe result-set read (`query_instance`, §2.3), and
peacock's state-transfer push (`updateModelContext` / the A2UI data-sync
equivalent, §2.4); everything else is composition of existing parts. peacock
is the new codebase, and its scope — render, host, embed, synchronize — is
deliberately small.

# 4. Conclusion

We have described an open, three-component architecture that reproduces the
agent-authored-dashboard capability of proprietary cloud tools on plain
DuckDB, while adding what those tools omit: native agent *consumption*,
portability across an MCP host, chat channels and structured output, and an
interactive surface whose state stays synchronized with the conversation.
Developed throughout on one running example — Northwind monthly revenue by
category — the design rests on a strict separation of concerns (escurel
virtualizes data into typed, access-controlled *structured data views*;
**peacock**, the new component, renders, hosts, and synchronizes; triton
adapts to chat) and on two ideas that recur at every layer: a report is a
composition of typed references to virtualized views read through one
parameterized, access-checked call; and a visualization's state is its
parameter vector, held authoritatively by the conversation. Keeping data and
credentials inside escurel, rendering inside peacock, and running state in
the conversation yields one trust boundary and one artifact that serves
humans and agents alike.

[^dives]: MotherDuck. *Dives* documentation and engineering notes.
  <https://motherduck.com/docs/key-tasks/ai-and-motherduck/dives/>;
  <https://motherduck.com/blog/claude-code-plus-dives-equals-any-data-ui/>.

[^quack]: DuckDB. *Quack: a remote protocol for DuckDB.*
  <https://duckdb.org/quack/>;
  <https://duckdb.org/2026/05/12/quack-remote-protocol>. Beta; first
  production release targeted for DuckDB v2.0 (~Sept 2026).

[^escurel]: DataZooDE. *escurel* — multi-tenant knowledge base with
  external instance backends (SQL-view / document virtualization over
  DuckDB `vss`/`fts`), `InstanceBackend` trait, `backend_ref` overlays,
  fail-closed page-level ACL. Project specification, 2026.
  <https://github.com/DataZooDE/escurel>.

[^triton]: DataZooDE. *triton* — single-binary multi-protocol agent-ingress
  gateway (MCP/A2A/REST + chat channels), A2UI surface mapper with PNG
  rasterization and HMAC-signed interaction tokens. Project specification,
  2026. <https://github.com/DataZooDE/triton>.

[^skillinst]: Rosskopf, J. *Skill::instance — progressive disclosure for
  agent memory.* DataZoo research note, 2026. A skill is a typed template
  page; instances conform to it; the type vocabulary is the set of skill
  pages.

[^mcpapps]: Model Context Protocol. *MCP Apps extension* (SEP-1865; first
  official MCP extension, 2026): servers return a `ui://` HTML resource
  rendered in a sandboxed iframe, with `callServerTool` /
  `updateModelContext` interaction over `postMessage`.
  <https://blog.modelcontextprotocol.io/posts/2026-01-26-mcp-apps/>.

[^a2ui]: Google. *A2UI* — framework-agnostic declarative JSON for
  agent-generated UI; v0.9 (Apr 2026) adds *Custom Component Catalogs* for
  domain components such as charts, and client-to-server data syncing.
  <https://developers.googleblog.com/a2ui-v0-9-generative-ui/>;
  <https://a2ui.org/guides/custom-components/>.

[^vega]: Vega-Lite — a high-level declarative grammar of interactive
  graphics. *vl-convert* renders Vega-Lite to SVG/PNG as a self-contained
  Rust library (no Node.js, no network).
  <https://github.com/vega/vl-convert>.

[^ggsql]: Posit. *ggsql* — a SQL extension for declarative visualisation
  based on the Grammar of Graphics; the `ggsql-duckdb` community extension
  adds `VISUALISE … DRAW …` clauses to DuckDB and compiles them to a
  Vega-Lite specification (`spec` output mode). Alpha (v0.3.0) at the time
  of writing. <https://ggsql.org/>;
  <https://github.com/posit-dev/ggsql-duckdb>.

[^northwind]: *Northwind* — Microsoft's long-standing sample trading
  database (customers, orders, order details, products, categories), widely
  used as a neutral analytics example; here as a stand-in for an operational
  store virtualized read-only by escurel.

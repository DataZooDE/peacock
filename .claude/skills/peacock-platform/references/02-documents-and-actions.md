# 02 — Documents and skill-declared actions

Any escurel INSTANCE page can be opened as a **document** — the target
of a chat reply's `sources` references. The record type's escurel
SKILL page declares how it renders and what a reader may do; nothing
per-type is hardcoded in peacock or in any consumer.

## The reserved `document` pseudo-report

`render_report` with `report_id: "document"` and params
`{skill, id}` (both slug-validated before any read; it intercepts
BEFORE authored-skill resolution, so an authored page named `document`
never shadows it):

- If the target's skill page declares a **viewer**, the render
  DELEGATES to that authored report with `{<param>: <id>}`.
- Otherwise a **generic view** renders: the page's own frontmatter as
  facts (structural keys excluded) + the markdown body + a 20-event
  timeline.
- Either way `structuredContent.document = {skill, id, actions}` rides
  the artifact, and the resource URI is
  `ui://peacock/document?skill=<skill>&id=<id>`.

## `viewer:` and `actions:` on the skill page

```yaml
---
type: skill
id: account
# …schema fields…
viewer: { report: customer-report, param: account }
actions:
  - name: propose-nba              # slug — the wire id sent back on click
    kind: prompt
    label: Propose next best action
    prompt: "whats the next best action for {id}?"
  - name: renewal-at-risk
    kind: event
    label: Flag renewal at risk
    event: follow_up               # escurel capture_event label_skill
    title: "{id}"
    body: "renewal at risk (flagged from the customer document)"
---
```

- Placeholders: `{id}` and `{frontmatter.<key>}` (string values only;
  a missing key is an author error naming the skill page).
  Substitution is SERVER-side — the client only ever receives final
  strings, and event `title`/`body` never ship to the client at all.
- `kind: prompt` → the rendered document shows a button; clicking
  posts `{type:"mcp:prompt", text}` to the HOST, which sends the text
  as a **new user turn** (same governance as typing).
- `kind: event` → clicking calls **`emit_document_event`**
  `{skill, id, action}`: peacock re-reads the skill page (the named
  action must exist with `kind: event`), re-reads the instance as the
  caller (existence + ACL), substitutes the templates, and
  `capture_event`s with the caller's bearer — driving the tenant's
  webhook/worker chains with **no agent hop**. Prompt-named, undeclared
  or smuggled emits fail closed; nothing is captured.

## Navigation

`[[skill::id]]` wikilinks in rendered bodies are navigable when both
segments are slug-shaped: a click re-renders the iframe as that
document (a fresh absolute `document` render — the drill pattern) and
pushes an `updateModelContext` view-state record.

## How replies reference documents (the consumer side)

An agent that writes instance pages surfaces them as a `sources`
component (see the triton-platform skill, `references/02`): items carry
`ui://peacock/document?skill=…&id=…` one level down, hosts open them
on click only. The agent needs to know nothing about viewers or
actions — the skill page is the contract.

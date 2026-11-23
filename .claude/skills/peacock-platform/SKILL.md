---
name: peacock-platform
version: 0.5.0
description: Use when building an application that integrates with Peacock, the DataZoo report renderer + MCP-App host — authoring escurel report skills it renders (params/data/instances/views/specs frontmatter), referencing reports from an agent's surface (render_report button + ui://peacock resource), opening instance DOCUMENTS from chat replies (the reserved `document` pseudo-report, skill-page `viewer:`/`actions:`, emit_document_event), theming a deployment (PEACOCK_BRAND_CSS, --pk-* tokens, get_theme), or writing no-mock integration tests (PeacockProcess, NorthwindEscurel). Triggers on phrases like "render a report", "report skill page", "ui://peacock", "render_report", "instance report", "document view", "viewer frontmatter", "actions frontmatter", "emit_document_event", "get_theme", "PEACOCK_BRAND_CSS", "pk tokens", "anofox chart", "roles spec", "dashboard.sql", "peacock author compile", "PeacockProcess". DO NOT use for peacock-internal work (the composer, the chart engine bridge, the guardrails, the iframe runtime) — that is a PR against the peacock repo itself, not consumer-facing.
---

# peacock-platform — build apps that integrate with Peacock

You are helping someone integrate with **Peacock**, the DataZoo
platform's stateless **report renderer + MCP-App host**. Peacock
compiles an **escurel report skill + params** into one artifact — A2UI
layout + anofox-visualization charts + `structuredContent` + an on-demand themed
PNG — and serves it on four surfaces (MCP App `ui://`, chat-via-Triton,
structured JSON, embedded). It reads **ACL-checked rows and pages from
escurel only** (the caller's bearer forwarded per request); it holds no
database credentials and constructs no SQL.

Three consumer roles, most apps are one of them:

- **Report author.** You write escurel *pages*: `query` pages over
  `sql_view`s (the data), and report skill pages (`render: a2ui` +
  params/data/instances/views/specs). Rendering a new chart is
  authoring, not deploying. → `references/01`.
- **Agent / surface integrator** (the agent-template case). Your agent
  **references** reports — a `render_report` button + a
  `ui://peacock/<id>?<params>` resource in its surface; Triton
  dispatches and proxies, Peacock renders. Your agent NEVER calls
  Peacock and forwards it no bearer. Chat replies can also reference
  the documents they wrote (`sources` → the `document` pseudo-report).
  → `references/02`, `references/03`.
- **Operator.** You register Peacock's tools with Triton and configure
  the deployment's brand. → `references/03`, `references/04`.

This skill is **read-only documentation**. Anything that requires
changing Peacock itself (a new view kind, a guardrail change, an iframe
behaviour) is a **PR against the peacock repo**.

## How this skill is installed

The peacock repo is checked out locally and this skill directory is
**symlinked** into the consumer repo's `.claude/skills/`:

```sh
ln -s ../path/to/peacock/.claude/skills/peacock-platform \
      .claude/skills/peacock-platform
```

The peacock repo's checked-out git ref is the version pin. Check
`VERSION` and `CHANGELOG.md`.

## Progressive-disclosure index

| File | Read when… |
|---|---|
| `references/00-what-is-peacock.md` | First contact. The escurel · peacock · triton shape, the four surfaces, the trust boundary. |
| `references/01-report-skills.md` | Authoring a report: the skill-page frontmatter (params / data / instances / views / specs), the query-page data path, validation + scaffolding. |
| `references/02-documents-and-actions.md` | Opening RECORDS from chat: the reserved `document` pseudo-report, `viewer:`/`actions:` on skill pages, `emit_document_event`, wikilink navigation. |
| `references/03-integration-wiring.md` | Wiring an agent/frontend: Triton registration keys, the `ui://` resource contract, the tool-result shape, PNG delegation, the iframe host verbs. |
| `references/04-theming.md` | Branding a deployment: `--pk-*` tokens, `PEACOCK_BRAND_CSS`, host ⊕ brand composition, `get_theme` — peacock owns ALL theming. |
| `references/05-test-harness.md` | No-mock integration tests: `PeacockProcess`, `NorthwindEscurel`, the fixtures pattern. |

## Hard prohibitions

- **Agents never call Peacock.** An agent emits *references* (the
  button + the `ui://` resource); Triton routes them. The agent holds
  no Peacock credential and forwards its escurel-scoped bearer to
  escurel **only**.
- **No SQL, anywhere.** Data reaches a report exclusively through
  authored `[[query::*]]` pages (`query_instance` — `{{target}}`
  allow-listed, `:params` prepared-bound, ACL-checked per caller).
  There is nothing to inject SQL into.
- **No theming outside Peacock.** Rendering aspects — charts, cards,
  iframes, brand chrome — belong here; protocol adapters + auth belong
  to Triton. Don't add brand config to a Triton manifest (the old
  `theme:` block is gone) or to your agent.
- **No hardcoded per-record-type rendering.** What a document shows
  and offers comes from its escurel SKILL page (`viewer:`,
  `actions:`) — authored knowledge, not consumer code.

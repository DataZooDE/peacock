# 01 — Authoring report skills

A report is an **escurel skill page** with `render: a2ui`. Peacock
resolves it on demand (`[[skill::<report_id>]]` — no registry) and
parses the frontmatter. Canonical parser:
`crates/peacock-core/src/skill.rs::ReportSkill::from_frontmatter`.

## The frontmatter

```yaml
---
type: skill
id: monthly-revenue
render: a2ui
description: Monthly revenue by category.
params:                       # the render's typed parameter schema
  category: { type: string, default: "ALL" }
data:                         # alias → an authored query page
  rows: "[[query::revenue_by_month]]"
instances:                    # alias → a parameterized instance page
  acct: "[[account::{account}]]"   # {param} substitutes from the ABSOLUTE vector
views:                        # the layout, in order
  - { kind: kpi,   data: rows, agg: sum, field: revenue, label: "Total" }
  - { kind: vega,  data: rows, spec: rev_bar }
  - { kind: table, data: rows }
  - { kind: frontmatter, instance: acct, keys: [name, status], label: Account }
  - { kind: markdown,    instance: acct }
  - { kind: timeline,    instance: acct, limit: 10 }
specs:                        # named Vega-Lite specs the vega views reference
  rev_bar: { mark: bar, encoding: { … } }
---
The narrative body (shown to authors, searchable).
```

- `kind ∈ {kpi, vega, table, markdown, frontmatter, timeline}` — the
  first three read `data:` aliases (query rows), the last three read
  `instances:` aliases (an escurel record). `frontmatter` needs a
  non-empty `keys:`; `timeline` shows the page's PROCESSED events
  (escurel `list_events`, oldest first — capture+assign to appear).
- **Data path**: a `data:` alias names a `[[query::<id>]]` page — an
  authored query with declared params, `target: [[<sql_view>::<inst>]]`
  and `{{target}}`/`:param` binding. See the escurel-platform skill
  (`query_instance`). No SQL ever appears in a report page.
- **Mixed reports bind the WHOLE param vector to every query** —
  escurel rejects undeclared params, so a report mixing rows and
  instances needs its query pages to declare the shared params.
- **Instance ids are slug-validated** after substitution (no `..`, no
  namespaces) before any escurel read; a smuggling param is a
  Validation error, never a partial artifact.
- Vega-Lite specs pass a **guardrail** (a safe subset); rows are capped
  (`max_rows`, default 10k) — an oversize view refuses to render.

## Validate + scaffold

The `peacock` binary's author tooling reuses the real parser and
guardrails:

```sh
peacock author validate path/to/report.md    # closed-check before seeding
peacock author scaffold my-report            # a minimal valid template
```

Cross-references are checked (every view's alias exists, every spec
named by a view exists, param defaults type-check).

## What a render returns

`structuredContent` carries `rows`, `param_schema`, `current_params`
(the view state — a consuming agent re-drills by changing params), and
`instances` (the typed record contract `{skill, id, page_id, facts,
markdown, events}` per alias) when the report declares them. The PNG
(chart, or the instance CARD for chartless instance reports) is themed
— `references/04`.

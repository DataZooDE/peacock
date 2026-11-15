# 01 — Authoring report skills

A report is an **escurel skill page** with `render: a2ui`. Peacock
resolves it on demand (`[[skill::<report_id>]]` — no registry) and
parses the frontmatter. Canonical parser:
`crates/peacock-core/src/skill.rs::ReportSkill::from_frontmatter`.

## The frontmatter

```yaml
---
kind: skill
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
  - { kind: anofox, data: rows, spec: rev_bar, spec_single: rev_line }
  - { kind: table, data: rows }
  - { kind: frontmatter, instance: acct, keys: [name, status], label: Account }
  - { kind: markdown,    instance: acct }
  - { kind: timeline,    instance: acct, limit: 10 }
specs:                        # named anofox chart specs the chart views reference
  rev_bar:
    title: Revenue by category
    roles:                    # [column, ROLE, display name?] — column = name or 0-based index
      - [month, XAXIS]
      - [category, CATEGORY]
      - [revenue, BARCHART_STACKED, Revenue]
  rev_line:                   # used instead when one CATEGORY series remains (a drill)
    roles: [[month, XAXIS], [category, CATEGORY], [revenue, LINECHART, Revenue]]
---
The narrative body (shown to authors, searchable).
```

- `kind ∈ {kpi, anofox, table, markdown, frontmatter, timeline}` — the
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
- **Charts are anofox-visualization role specs** (peacock#44). Any role
  from anofox's registry works (`BARCHART`, `BARCHART_STACKED`,
  `LINECHART`, `AREACHART`, `SCATTER` + `SIZE`, `PIE`, `DONUT`, `HEATMAP`
  with `YAXIS`, `HISTOGRAM`, `DENSITY`, `BOXPLOT`, `KPI`, `XLINE` …). The
  **guardrail** rejects unknown roles, layout / input / chrome roles inside
  a panel, `LINK` / `IMAGE` / download roles, and any authored `rows`,
  `theme`, `plot` or `url` key (peacock injects rows and theme). Allowed
  keys: `roles`, `title`, `width`, `height`, `max_categories`,
  `max_line_points`. Rows are capped (`max_rows`, default 10k).
- **Removed (hard cut):** `kind: vega` views, Vega-Lite specs (`mark`,
  `encoding`, `$schema`) and `geom:` statistical specs fail with an error
  naming the migration. Rewrite each as a `roles:` spec.

## Whole dashboards (`peacock author compile`)

An anofox `dashboard.sql` can be a report. Compile it at **author time**:

```sh
peacock author compile dashboard.sql --report nw-dashboard \
  --source order_lines --target "[[nw_order_lines::eu]]" \
  --param from:date=1997-01-01 --param to:date=1997-12-31 --out compiled/
```

It runs anofox's linter as a **hard gate** (e.g. `8::COL` inside a chart
SELECT — anofox would silently draw nothing — fails with
`layout/inline-layout-role` and the statement number), then emits one
escurel query page per statement (`compiled/query/<report>-q<n>.md`:
SELECT-only, the `--source` table → `{{target}}`, `getvariable('x')` →
`:x`) and the report skill (`compiled/<report>.md`: params, the script, a
manifest keyed by each statement's content hash). Seed all of them into
escurel. At render peacock reads only those pages (by reference, typed
params) — never SQL — and fails if a compiled panel is not drawn.
Setup statements (CREATE/INSERT/SET/COPY) are rejected: setup belongs in
the escurel view.

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

`render_report` takes optional `variant` (`light` | `dark` — use dark in a
dark client; brand colours are lifted to WCAG AA on the dark card) and
`locale` (e.g. `de-DE`); an unknown value is a validation error. The
result's `_meta` carries `svgs` (the painted anofox chart(s), what the
`ui://peacock` runtime shows — hover for values, click a series to drill),
`png_base64` (the first chart, for chat cards) and `vega_specs` (a minimal
Vega-Lite **derived** from bar/line/area/scatter/pie/donut panels, for
native-chart hosts like Teams and Gemini Enterprise; other charts have
none). `GET /ui/<report>` serves the runtime standalone.

`structuredContent` carries `rows`, `param_schema`, `current_params`
(the view state — a consuming agent re-drills by changing params), and
`instances` (the typed record contract `{skill, id, page_id, facts,
markdown, events}` per alias) when the report declares them. The PNG
(chart, or the instance CARD for chartless instance reports) is themed
— `references/04`.

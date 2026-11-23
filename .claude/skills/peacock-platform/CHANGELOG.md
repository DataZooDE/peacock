# Changelog — peacock-platform skill

The skill version tracks the consumer-facing contract; the peacock
repo's checked-out git ref is the true version pin (see `SKILL.md` →
"How this skill is installed").

## 0.5.0 — render audit line, truncation on chat cards (peacock#46)

- Every render writes one `kind: "audit"` JSON line to stdout (FR-O-4) on
  every surface, failures included: report id, surface, host flavor,
  tenant/subject, param NAMES, `result`, `duration_ms`, `rows` and
  `truncated`. Never rows, values or tokens. See
  `references/03-integration-wiring.md` → "The audit line".
- `RenderOpts` gains `audit: Option<AuditScope>` (default: an `embedded`
  render). An embedding app therefore gets audit lines on ITS stdout; pass
  `audit: None` to opt out.
- A capped view's chart title gains "(Partial data: first N rows…)", so the
  cut shows on Teams native charts (`_meta.chart.title`) and on every PNG
  (Google Chat, Teams fallback image).
- Test support: `PeacockProcess::stdout_lines()` / `audit_lines()` /
  `wait_for_audit_lines()`; the child's stdout is drained continuously.

## 0.4.0 — `_meta.vega_specs` removed (triton#382)

- `render_report` no longer sends the derived Vega-Lite (`_meta.vega_specs`
  on MCP, `vega_specs` in the HTTP render response), and `Artifact` loses
  its `vega_specs` field. Native-chart hosts use `_meta.chart`; Triton no
  longer reads `vega_specs` either. Mosaic mode keeps its own derived spec.

## 0.3.0 — neutral chart summary and row truncation (triton#375, peacock#46)

- `render_report`'s `_meta` gains `chart`: the renderer-neutral summary of
  the primary chart (Triton upstream contract §4b, `chart` v1) that Triton
  maps to native charts. `_meta.vega_specs` is **deprecated** and ships for
  one more release.
- Through Triton's MCP ingress, peacock's chart keys (`png_base64`, `svgs`,
  `chart`, `vega_specs`) now reach card hosts (MCP Apps clients); `ui`
  reaches every client (triton#375, allow-list per triton#383).
- A view over the row cap is **capped, not refused**. `structuredContent`
  gains `truncated: { rows_shown, rows_total?, limit, view }` whenever
  escurel's server cap or peacock's `max_rows` clipped a view; the text
  content and the `ui://` runtime ("Partial data" badge) say so.
- Test support: `oversized_fixtures()` / `NW_REPORT_OVERSIZED` — a Northwind
  report over escurel's row cap.

## 0.2.0 — anofox-visualization charts (BREAKING, peacock#44)

- Charts are `kind: anofox` views over anofox **role specs**
  (`roles: [[column, ROLE, name?], …]`). `kind: vega` views, Vega-Lite
  (`mark`/`encoding`) and `geom:` specs are rejected with a migration error.
- `render_report` takes `variant` (light|dark) and `locale`; `_meta` gains
  `svgs` (the painted charts); `vega_specs` is now a **derived** minimal
  Vega-Lite for native-chart hosts (bar/line/area/scatter/pie/donut only).
- Every chart of a report is painted, not only the first.
- New: whole anofox `dashboard.sql` reports via `peacock author compile`
  (lint gate → one escurel query page per statement + the report skill).
- `GET /ui/<report>` serves the `ui://peacock` runtime standalone; the
  runtime shows the interactive SVG (hover values, click-to-drill).

## 0.1.1 — escurel `kind:` release

Follows the escurel BREAKING release (see escurel's `docs/deploy/consumer-rollout.md`):

- Every page example uses `kind: skill|instance` (the `type:` page-kind key was
  removed by the engine: `frontmatter_type_removed`). `peacock author scaffold`
  now emits `kind:`, and peacock's saved-render instances are written with
  `kind: instance`.
- The `actions:` objects documented in `02` already match escurel's wire shape
  `{name, kind: prompt|event, label, event?, prompt?}` (list_skills returns them
  as such; plain skill-id strings are rejected by the engine).
- A skill page may now declare `folder`, `role`, `tags` and the OKF keys
  (`title`, `resource`, `generated`, `verified`, `status`, `stale_after`,
  `sources`): peacock ignores them, the escurel workbench shows them.

## 0.1.0 — initial release

The consumer-facing contract for the report renderer + MCP-App host,
current as of the document-view + theming era (peacock#13–#18):

- `00` — the escurel · peacock · triton shape, four surfaces, the
  trust boundary (forwarded caller bearer; no SQL, no DB creds).
- `01` — authoring report skills: params / data (`[[query::*]]`) /
  instances (`[[skill::{param}]]`) / views (kpi, vega, table,
  markdown, frontmatter, timeline) / specs; validation + scaffolding;
  the mixed-report shared-param rule; slug validation.
- `02` — the reserved `document` pseudo-report, skill-page `viewer:` +
  `actions:` (prompt | event), `emit_document_event`, wikilink
  navigation, how chat replies reference documents (`sources`).
- `03` — Triton wiring (the four registration keys), the
  reference-never-call agent pattern, the tool-result + `ui://`
  contract (params ride the URI), PNG delegation, the three iframe
  host verbs (`mcp:callServerTool` / `mcp:updateModelContext` /
  `mcp:prompt`).
- `04` — theming: peacock owns ALL of it; the `--pk-*` token table,
  host ⊕ brand resolution, `PEACOCK_BRAND_CSS`, `get_theme`.
- `05` — the no-mock harness (`PeacockProcess`, `NorthwindEscurel`),
  patterns and gotchas.

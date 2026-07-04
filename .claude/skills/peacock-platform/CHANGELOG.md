# Changelog — peacock-platform skill

The skill version tracks the consumer-facing contract; the peacock
repo's checked-out git ref is the true version pin (see `SKILL.md` →
"How this skill is installed").

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

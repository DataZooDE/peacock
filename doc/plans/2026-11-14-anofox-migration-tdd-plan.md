# TDD plan — migrate the render engine to anofox-visualization (#44)

Status: implemented on `feat/anofox-engine` · 2026-11-14 · tracks issue #44 (see *Implementation status* at the end)

## Decisions taken

| Topic | Decision |
|---|---|
| Scope | All five phases in **one PR** (one CI run). Each phase is one or more commits, and every commit is red → green. Push only when the whole gate is green locally. |
| Dependency | Git dep `anofox-visualization = { git = "https://github.com/DataZooDE/anofox-visualization", tag = "v2026.11.12", default-features = false }`. No features (`serve`/`auth`/`wasm` off). |
| Upstream | Don't wait. Build against the v2026.11.12 API. peacock guards the known gotchas itself. The upstream asks are filed as anofox issues in parallel. |
| Dark mode | Auto-lift the brand accent until it has ≥ 4.5:1 contrast on the dark card. No `--pk-dark-*` tokens yet. |
| `kind: vega` / `kind: stat` | **Hard cut.** Every fixture and seed moves to `kind: anofox`. `kind: vega` and `geom:` specs become a clear render error. This needs a companion escurel PR for `demo/seed/skills/supplier-risk-report.md`. |
| Browser E2E | `scripts/verify-ui.sh` drives `uvx rodney` against a peacock-served page, like escurel's `scripts/verify-demo.sh`. It runs as its own CI step. |

## The anofox seams we build on (v2026.11.12)

- `anofox_visualization::render_spec_checked(spec_json: &str) -> Result<String, RenderError>` renders one panel from `{rows, roles, …}` to SVG.
- `host::render_spec_checked` + `host::RenderLimits { max_rows, max_spec_bytes, min_dim, max_dim }` is the same render with caps.
- `dashboard::DataProvider { execute(&mut, sql), query(&mut, sql) -> Vec<(String, Vec<Value>)> }` and `render_dashboard_svg(script, &mut dyn DataProvider, &DashboardOptions) -> Result<String, String>`.
- `sql::plan(script) -> Vec<Panel>` is pure: it splits the script into statements and rewrites the casts.
- `theme::resolve_json` resolves a theme. `page::render(&StaticPage) -> String` produces a static HTML page with inline SVG and a hover script.

## Realistic E2E journeys (the Definition of Done)

Every journey runs against **real** components: `NorthwindEscurel` / `EscurelProcess`, `PeacockProcess`, the `triton` binary and Chromium via rodney. No fakes at any boundary.

| # | Journey (what a user does) | Test | Phase |
|---|---|---|---|
| J1 | An analyst in an MCP host (Claude/ChatGPT) asks for "Northwind revenue by category 1997". The host calls `tools/call render_report`, then `resources/read ui://peacock/…`. They see a themed stacked bar, a donut and a $2,986-ish KPI that **matches the fixture**. | `peacock-server/tests/suite/anofox_e2e.rs::mcp_host_renders_northwind_with_anofox` | 1 |
| J2 | The same request via Triton. The chat card has a PNG, and `_meta.vega_specs` carries a derived spec. | `triton_upstream.rs::triton_card_carries_png_and_derived_vega` (real `triton`) | 2 |
| J2b | Alice types into **Teams**. Her agent answers with a Northwind report, and the Teams card shows a **native** `Chart.VerticalBar` with an anofox PNG fallback. | `triton_upstream.rs::teams_message_gets_native_chart_from_anofox_report` (real `triton` + `FakeBotFramework`) | 2 |
| J3 | A backend calls the structured surface `POST /render`. It gets rows, `anofox_specs` and an SVG for **every** chart, not only the first. | `http_surface.rs::structured_render_returns_every_chart` | 1 |
| J4 | Parity: MCP-App, chat, structured and embedded produce the same artifact (ACC-1). | `parity.rs` (existing, extended to anofox) | 1 |
| J5 | Tenant `company-a` on a Copilot host, with the client in dark mode. The chart uses the brand palette, the accent is lifted to ≥ 4.5:1, numbers are in `de-DE` format, and rendering twice gives byte-identical SVG/PNG (ACC-8). | `anofox_theme_e2e.rs::branded_dark_de_render_is_accessible_and_deterministic` | 1 |
| J6 | A report author writes `dashboard.sql`, runs `peacock author compile`, seeds the emitted query pages into escurel and opens the report. The **whole** dashboard renders. A dialect slip fails the compile instead of silently dropping panels. | `peacock-bin/tests/suite/author.rs::compile_then_render_whole_dashboard` + `compile_rejects_layout_role_in_data_statement` | 3 |
| J7 | An analyst hovers a bar in the `ui://peacock` iframe and sees a tooltip, clicks a category and gets a fresh render with **absolute** params, and the chart changes. | `scripts/verify-ui.sh` (rodney) | 5 |
| J8 | After the cut, an old `kind: vega` skill fails with a clear message, and the workspace no longer contains `peacock-rasterizer`/`peacock-ggplot`. | `peacock-core/tests/suite/legacy_rejected.rs` + `peacock-core/tests/suite/no_credential_surface.rs` (extended grep) | 4 |

**Overall DoD:** J1–J8 (incl. J2b) green locally; the four-step pre-push gate (`fmt`, `clippy -D warnings`, `test --all-targets` + `--doc`, `build --release`) green; `scripts/verify-ui.sh` exits 0; then one push and one green CI run.

---

## Phase 1 — `kind: anofox` per-chart view

### 1.1 Dependency and backend trait

- **Red:** `peacock-core/tests/suite/compose_unit.rs::anofox_view_composes_an_anofox_component`. A skill with `- { kind: anofox, data: rev_by_cat, spec: rev }` and `specs.rev.roles: [[0,"XAXIS"],[1,"CATEGORY"],[2,"BARCHART_STACKED"]]` composes to the A2UI component `{kind:"anofox", svg, roles}`. Fails today: unknown view kind.
- **Green:**
  - Add the git dep.
  - Introduce `trait ChartBackend { fn compose(&self, spec, rows) -> Result<ChartOut>; fn rasterize(&self, out, scale, theme) -> Result<Png> }` in `peacock-core`.
  - Replace the `chart.get("geom")` dispatch (`compose.rs:195`) and the `attach_png` branches (`render.rs:186-206`) with that trait.
  - Add `AnofoxBackend`, which builds `{rows, roles, title, theme}` and calls `host::render_spec_checked` with `RenderLimits`.
- **Refactor:** move the Vega and stat paths behind the same trait. They are deleted in Phase 4, but the trait has to be proven with ≥ 2 impls first.

### 1.2 Guardrails (ACC-4)

- **Red** (`peacock-core/tests/suite/anofox_guardrails.rs`):
  - `unknown_role_is_rejected`: the role must exist in anofox's role table.
  - `link_role_is_rejected`: no `::LINK` hrefs.
  - `non_data_uri_image_is_rejected`.
  - `oversized_rows_are_capped_and_truncation_is_surfaced`: escurel caps at 10k rows and `RenderLimits` is lower; assert on `structuredContent.truncated`.
  - `svg_is_inert`: the output contains no `<script`, `foreignObject`, `href=` or non-`data:` URL.
- **Green:** `check_anofox_spec()`, plus a post-render SVG sanitizer assertion (defence in depth).

### 1.3 Artifact carries every chart

- **Red:** `render_northwind.rs::every_chart_is_rasterized`. A report with two anofox views gives `artifact.anofox_specs.len() == 2`, two SVGs, and one PNG per chart. Today only the first chart is rasterized.
- **Green:**
  - `Artifact { anofox_specs: Vec<Value>, svgs: Vec<String>, pngs: Vec<Vec<u8>> }`, using serde `skip_serializing_if` so the wire stays additive.
  - Keep `png` as `pngs[0]` for back-compat on the Triton card.

### 1.4 PNG via resvg with bundled OFL fonts

- **Red:** `anofox_png.rs::png_uses_bundled_font_not_fallback`. Render a title in Inter and compare glyph coverage/width against a fixture rendered with a known fallback. They must differ, and the result must be deterministic.
- **Green:**
  - A small `peacock-core::raster` module: resvg + `fontdb` loaded **only** from `include_bytes!` OFL fonts (Inter, Hanken Grotesk, DejaVu Sans for symbols). Resolve `data:` images only.
  - Port anofox `duckext/src/raster.rs` (~100 lines).
- **Note:** this module outlives `peacock-rasterizer`, which is deleted in Phase 4.

### 1.5 Theme bridge, dark auto-lift and locale

- **Red** (`peacock-theme/tests/theme.rs`):
  - `to_anofox_json_maps_every_token`: a table-driven test for each row of the mapping in #44 (`cat-1..N`→`palette`, `brand`→`colors.accent/info/link`, …).
  - `dark_variant_lifts_accent_to_aa`: `#6b3fa0` on the anofox dark card → contrast ≥ 4.5 and hue preserved; the expected value is about `#906fb8`.
  - `light_variant_leaves_compliant_accent_untouched`.
  - `locale_rides_the_theme`: `de-DE` → `1.234,56`.
  - `unmapped_grid_axis_tokens_fall_back_to_chart_line` documents the gap.
- **Green:**
  - `ThemeTokens::to_anofox_json(variant, locale)` and a `lift_contrast(fg, bg, 4.5)` helper (WCAG relative luminance; step lightness in HSL).
  - Host ⊕ brand composition stays in peacock.
- **Surface:** `render_report` accepts `variant: "light" | "dark"` (default light) and `locale` (falls back to the tenant default). Red: `mcp_surface.rs::render_report_accepts_dark_variant`.

### 1.6 Phase 1 DoD (J1, J3, J4, J5)

- Migrate `nw_revenue_by_category` in `peacock-test-support/src/northwind.rs` to `kind: anofox`. Fixture migration is a hard cut; the rest follows in Phase 4.
- J1, J3 and J5 are green, and `parity.rs` (J4) is green with anofox specs.

## Phase 2 — Triton native charts keep working

- **Red:** `peacock-core/tests/suite/derived_vega.rs`, table-driven over the roles `BARCHART`, `BARCHART_STACKED`, `LINECHART`, `AREACHART`, `PIECHART`, `DONUT`. Each gives a minimal Vega-Lite with inline `data.values` and `encoding.{x,y,color}.field`, with exactly the shape Triton's `chart_from_vega` / `beautify_vega_for_ge` read. Other roles (histogram, map, KPI) give **no** vega spec and only a PNG.
- **Green:** `derive_vega_lite(roles, rows) -> Option<Value>`, which fills `artifact.vega_specs` as a **derived** output.
- **Red, E2E (J2):** `triton_upstream.rs::triton_card_carries_png_and_derived_vega` against the real `triton` binary. Dispatch `render_report`, then assert that the A2UI card has an image and `_meta.vega_specs[0].mark == "bar"` with the encoding fields.
- **Red, E2E (J2b, the real Teams surface):** `triton_upstream.rs::teams_message_gets_native_chart_from_anofox_report`. It mirrors triton's own `msteams_report_image.rs`:
  - Run the real `triton` binary via `TritonProcess::spawn_with_env` with the `manifest-msteams-report-test.yaml` shape. `TRITON_STATIC_UPSTREAMS` sets `render_report=<real peacock>` and `assistant=<FakeAgent>`.
  - Use triton-tests' `FakeBotFramework` as the stand-in for Microsoft's Bot Connector. It is a real HTTP server for a third-party service, not the boundary under test.
  - `FakeAgent` plays the user's chat agent and answers with an A2UI `report` component for `nw_revenue_by_category`. It is also outside the boundary under test.
  - Alice POSTs a signed `message` activity to `/msteams/webhook`.
  - Assert that the captured outbound activity's Adaptive Card contains a `Chart.VerticalBar`/`Chart.VerticalBar.Grouped` element, built by triton's `report_vega_chart` → `find_vega_spec` → `chart_from_vega` from peacock's `_meta.vega_specs`.
  - Assert that its `fallback` image URL, when fetched from triton's img route, returns PNG bytes rendered by anofox.
- **Contract note:** triton's `find_vega_spec` matches a `{kind:"vega",spec}` component **or** `_meta.vega_specs`. The new `{kind:"anofox"}` component does not match, so `_meta.vega_specs` must always carry the derived spec for chartable roles.
- **Cross-repo:** a triton follow-up issue, "read neutral `{kind,x,y,series,rows}` instead of Vega-Lite". It is filed, not implemented here.

## Phase 3 — whole dashboards as reports

### 3.1 `peacock author compile`

- **Red** (`peacock-bin/tests/suite/author.rs`):
  - `compile_emits_one_query_page_per_statement`: the Northwind `dashboard.sql` gives N `[[query::nw_dash.<n>]]` pages. In each page, `getvariable('from')` becomes `:from`, the source view becomes `{{target}}`, and the page has `target: [[sql_view_instance]]`.
  - `compile_rejects_non_select`: `DELETE`/`CREATE`/`COPY` statements are refused, because escurel does not check the statement type.
  - `compile_rejects_layout_role_in_data_statement`: the spike's silent-drop slip (`8::COL` inside a chart SELECT) is a hard lint error with the line number.
  - `compile_writes_a_manifest`: `sha256(normalized planned SQL) → query ref`, plus the expected panel count.
- **Green:** `author compile` runs the anofox lint, then `sql::plan`, then the page emitter and writes the manifest. It sits next to scaffold/validate/preview in `peacock-bin/src/author.rs`.
- **Trust boundary:** SQL text exists only at **author time**, in `peacock-bin`. The render crates never construct or send it. Red: extend `no_credential_surface.rs` so `peacock-core`/`peacock-server` must not reference `sql::plan` or format SQL. See question Q-B.

### 3.2 `EscurelProvider`

- **Red** (`peacock-core/tests/suite/anofox_dashboard.rs`, real `NorthwindEscurel`):
  - `provider_sends_no_sql_to_escurel`: capture the outgoing `query_instance` requests through the wire-trace sink and assert that they carry only `ref` + typed params.
  - `provider_maps_statement_to_query_page_via_manifest`: unknown SQL means a hard error, not a guess.
  - `provider_refuses_setup_statements`: `execute()` returns an error.
  - `escurel_types_convert_to_anofox_values`: ISO dates become `DateTime`, ints/floats keep their type, NULL becomes `Na`, and the `Debug`-form type names are parsed.
- **Green:** `EscurelProvider: DataProvider`. `render_dashboard_svg` is called with `DashboardOptions { theme, variant, width, only, single }`.

### 3.3 Panel-count guard and params

- **Red:**
  - `rendered_panel_count_matches_manifest`: if anofox drops a panel, the render is an error.
  - `dropdown_input_maps_to_report_param`: `::DROPDOWN`/`::DATERANGE` map to `params:`, and a drill is a fresh absolute-params render (ADR-P7).
  - `only_single_renders_one_panel_as_chat_card`.
- **DoD (J6):** `compile_then_render_whole_dashboard`. Compile, seed the pages into a real escurel, render, and assert the KPI against the fixture, all panels present, and the ACC-2/11 grep green.

## Phase 4 — retire the old engines (hard cut)

- **Red:**
  - `legacy_rejected.rs::kind_vega_is_a_clear_error`: the message names the migration.
  - `legacy_rejected.rs::geom_spec_is_a_clear_error`.
- **Port the parity cases:** the 39 cases in `peacock-rasterizer/tests/parity.rs` and the `peacock-ggplot` cases become `peacock-core/tests/suite/anofox_parity.rs`. Each case asserts that the role output renders, plus structural SVG facts (mark count, legend entries, axis labels), not pixel equality. `StatSpec` geoms map to `::HISTOGRAM`/`::DENSITY`/`::BOXPLOT`/ECDF.
- **Fixtures:** migrate every `kind: vega` view in `northwind.rs` (7) and `deliveries.rs` (1).
- **Green:**
  - Delete `peacock-rasterizer`, `peacock-ggplot`, `apply_chart_theme` and the `ggplot` cargo feature, along with the workspace entries for `ggplot-rs`/`vl-convert`.
  - Retarget the instance-card PNG, which currently uses `peacock_rasterizer::render_instance_card_*`, to anofox KPI/table roles. Its own red test is `instance_card_png.rs`, kept and repointed.
- **Cross-repo:** an escurel PR migrates `demo/seed/skills/supplier-risk-report.md` to `kind: anofox`. Merge it together with this PR.
- **Docs:**
  - `CLAUDE.md` crate list and `.claude/skills/peacock-platform`.
  - ADR amendment: FR-V-1/ADR-P3 and FR-V-5/ADR-P10 now read "chart language = anofox roles; Vega-Lite = derived". Update `BRD.md`, `HLD.md` and `paper.md`.
  - Future-notes under `doc/notes/discovered/` (silent panel drop, PNG fonts, grid/axis tokens).
- **DoD (J8):** the legacy-rejected tests pass, the extended grep finds no `peacock_rasterizer`/`peacock_ggplot`, and the workspace builds without them.

## Phase 5 — interactive `ui://peacock` iframe

- **Red, Rust:**
  - `mcp_surface.rs::resources_read_serves_inline_svg_with_hover_script`: the resource HTML contains inline anofox SVG from `page::render` plus the hover script, with the PNG as the `<noscript>`/fallback. The CSP from `_meta.ui` must still allow it (no remote script).
  - `mcp_surface.rs::chart_click_maps_to_absolute_params`: the data attributes on the SVG marks give the next param vector.
- **Green:**
  - `iframe.html` embeds `page::render` output.
  - Clicking a mark calls `callServerTool("render_report", {report_id, params: absolute})`, then `updateModelContext`.
- **Red, browser (J7):** `scripts/verify-ui.sh`:
  1. Boot `NorthwindEscurel` + peacock (the prebuilt bin through `PEACOCK_BIN_DIR`, as in escurel's script).
  2. `uvx rodney start`, then `open http://127.0.0.1:$PORT/ui/nw_revenue_by_category`, the standalone HTTP fallback of the same resource.
  3. `rodney wait 'svg [data-role=BARCHART_STACKED]'` and `rodney hover` on the Beverages mark; `rodney assert` that the tooltip text contains `Beverages` and a `$` amount.
  4. `rodney click` on the Beverages chip/mark; `rodney waitidle`; `rodney assert` that `current_params.category === "Beverages"` and that the rendered total equals the fixture value for Beverages.
  5. `rodney screenshot` to `target/verify-ui/*.png` (a CI artifact); `rodney stop`.

  The script exits non-zero on the first failed assert.
- **CI:** a new step after `cargo test` that reuses the built binaries, with a timeout of about 5 min. `uvx` needs `uv` on the runner (`astral-sh/setup-uv`) and Chromium (`browser-actions/setup-chrome`).

## Commit order inside the single PR

1. `deps: add anofox-visualization v2026.11.12` (builds; no behaviour change)
2. `core: ChartBackend trait` (red → green 1.1)
3. `core: anofox guardrails` (1.2)
4. `types: artifact carries every chart` (1.3)
5. `core: OFL-font PNG raster` (1.4)
6. `theme: anofox bridge, dark auto-lift, locale` (1.5)
7. `test: Northwind anofox E2E J1/J3/J4/J5` (1.6)
8. `core: derived Vega-Lite for Triton` (Phase 2 + J2 + J2b)
9. `bin: author compile` (3.1)
10. `core: EscurelProvider + whole dashboards` (3.2, 3.3, J6)
11. `BREAKING: hard-cut kind: vega / geom; delete rasterizer + ggplot` (Phase 4, J8)
12. `server: interactive iframe + scripts/verify-ui.sh` (Phase 5, J7)
13. `docs: ADR amendment, CLAUDE.md, skill, discovered notes`

## Risks

- **anofox build time:** the core crate takes about 1.5 min in release; ggplot-rs is pinned by git rev upstream. Watch the 90-minute CI timeout.
- **BUSL-1.1 Additional Use Grant:** confirm it covers how peacock ships to customers (Phase 0, non-code).
- **anofox API churn:** pinned by tag, and upgrades are explicit PRs.
- **The hard cut couples peacock and escurel merges:** the PR description must say "merge together", the same pattern as #42.

## Resolved questions

- **Teams native chart proof:** the real Teams surface through the `triton` binary (J2b), using `FakeBotFramework` for Microsoft's side.
- **`author compile` and principle 3:** allowed, but only in `peacock-bin` author tooling. The ACC-2/11 grep is extended so `peacock-core`/`peacock-server` never reference `sql::plan` or format SQL. Reword principle 3 in `CLAUDE.md` to "the **render path** constructs no SQL string; author-time compile in `peacock-bin` rewrites author-supplied SQL into escurel query pages".

## Implementation status

All eight journeys pass against real components: J1–J5 and J8 in `cargo test`, J6 through the real
`peacock` binary, and J7 through `scripts/verify-ui.sh` in headless Chrome.

Deviations from the plan, and why:

- **No `ChartBackend` trait.** The hard cut leaves one engine, so a trait with a single
  implementation would be ceremony. The boundary is the `peacock-core::chart` module (and
  `dashboard` for whole dashboards).
- **Commit order.** The core hard cut landed together with the anofox view (one commit), not as a
  separate step 11. Keeping both dialects alive in between would have meant migrating every test
  twice.
- **J2 reads Triton's dispatch result.** Triton's MCP surface rebuilds `_meta`, so the PNG, SVG and
  derived spec are asserted on `POST /v1/tools/render_report`. The Teams path (J2b) reads them the
  same way inside Triton.
- **Instance card and Triton KPI tiles.** Both are rendered by anofox (`MARKDOWN` under a `TITLE`
  bar; `TEXT_LARGE` with a `LABEL`) and laid out by a small grid composer. anofox's single-panel
  API cannot draw `TABLE`/`RECORD` panels from JSON rows.
- **Not done: truncation surfacing.** The existing `max_rows` render error still bounds rows, and
  anofox's `RenderLimits.max_rows` is set to the same 10k.

Still open (non-code or cross-repo):

- Phase 0: alignment with the anofox maintainer, the BUSL-1.1 Additional Use Grant check, and
  filing the upstream asks (stable API / lean feature, theme registry, `chart.grid`/`chart.axis`,
  custom-accent contrast checks, an error instead of a silent drop).
- escurel PR: migrate `demo/seed/skills/supplier-risk-report.md` (`kind: vega`) to `kind: anofox`.
  Merge it together with this PR.
- triton follow-up issue: read a neutral `{kind, x, y, series, rows}` chart summary instead of
  Vega-Lite.

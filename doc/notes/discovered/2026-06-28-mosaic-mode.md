# Mosaic/vgplot big-data cross-filter mode — the peacock-side contract

**Date:** 2026-06-28
**Scope:** `peacock-core` (`RenderOpts`, `compose`, `guardrail`),
`peacock-types` (artifact shape, unchanged surface).

## What this is

BRD §7 defers "Mosaic/vgplot live big-data cross-filter" as a *second* render
mode behind the default inline-data + re-render-on-drill model. This note
records the peacock-side contract that is now implemented; the **JS/Flutter
Mosaic client runtime that consumes it is the follow-up and is NOT in this
repo**.

## The problem it solves

By default peacock inlines a view's escurel rows into the Vega-Lite spec
(`data.values`). Above `RenderOpts.max_rows` (NFR-P-3) it refuses to render
(bounded `Render` error). For genuinely oversized result sets we want neither
"inline a million rows" nor "fail" — we want to hand the client a spec that
streams from escurel.

## The contract

- `RenderOpts.mosaic_threshold: Option<usize>` — default `None` (inline model
  unchanged for every view). When `Some(n)` and a **chart (vega) view's** row
  count exceeds `n`, that view is emitted in Mosaic mode instead of inlining.
- A Mosaic-mode chart is an A2UI component:
  ```json
  {
    "kind": "mosaic",
    "artifact": {
      "spec":   { "mark": "...", "encoding": { ... } },   // vgplot spec, NO data
      "source": { "connector": "escurel", "query_ref": "<id>", "params": { ... } },
      "row_count": <n>
    }
  }
  ```
  The `spec` is derived from the report's authored Vega-Lite spec (same mark +
  encodings), with **no `data.values`** — the rows are not inlined. The
  `source` is the escurel-owned data **source** reference: a typed `query_ref`
  plus the bound (absolute) params. It is never SQL and never a URL.
- `structuredContent` keeps the view state (`current_params`) and drops the big
  inline rows (`rows: []`); the per-view `row_count` lives on the component.

## The guardrail invariant (the load-bearing bit)

`guardrail::check_mosaic_source` enforces the *single* exception to
inline-data-only:

> The only permitted non-inline data source is an **escurel-owned** query
> reference — `{ connector: "escurel", query_ref, params }`. Any other
> connector, a blank/missing `query_ref`, or a `url`/`loader`/`expr`/`signal`
> escape hatch is rejected. escurel stays the sole data path and ACL boundary;
> peacock still constructs no SQL and holds no credential.

Normal charts are unaffected: their authored spec still goes through
`check_vega_spec` and their rows are still injected inline.

## How to recognise it next time

- A report over a large view that currently 500s with "refusing to render
  unbounded" is a candidate for `mosaic_threshold` (set it *below* `max_rows`).
- The mosaic threshold lifts the oversize bound **only for chart views**.
  A `table`/`kpi` view over `max_rows` still errors — those have no streaming
  client and inlining them is exactly what the bound exists to prevent.
- If you add a new connector to the source ref, update `check_mosaic_source`
  (the allow-list is `connector == "escurel"` and nothing else).

## Follow-up (out of scope here)

The Flutter/web Mosaic runtime that reads `kind: "mosaic"`, opens a live
escurel-backed data source from `source.query_ref` + `source.params`, and does
the in-browser cross-filter against the vgplot `spec`. peacock only produces
the contract above; it remains stateless and SQL-free.

# vl-convert is too brittle to build; peacock renders its Vega-Lite subset in pure Rust

**Symptom.** Adding `vl-convert-rs` (the Vega-Lite → PNG rasterizer the BRD
names) to the workspace fails resolution and compilation in several ways:

1. `aes`: `vl-convert-rs` embeds Deno/V8, whose `deno_crypto` pins
   `aes =0.8.3`, while escurel-index (via `kreuzberg` → `lopdf 0.41`) needs
   `aes ^0.8.4`. Irreconcilable; escurel's recent `main` merge (newer `lopdf`)
   introduced it, and `kreuzberg` is a non-disablable default feature of
   `escurel-server`, so peacock cannot drop it.
2. Isolating `vl-convert-rs` in its own standalone workspace then fails to
   **compile**: a fresh resolution on Rust 1.96 floats Deno's transitive deps
   too new — `tokio-stream` needs `--cfg tokio_unstable`; old `swc_config`
   needs pre-`serde_core`-split serde (`serde::__private`); `temporal_rs 0.1.2`
   needs `icu_calendar =2.1.0` but the icu family resolves lock-step to 2.2.x.
   Each pin uncovers the next; reproducing Deno's exact historical dependency
   set is fragile and would break on every toolchain bump.

**Decision (per the project owner: "if something is brittle, find a better
solution; else reimplement the brittle solution as spec in stable, high-
performance Rust with red/green TDD").** Drop `vl-convert` entirely. peacock
already restricts charts to a **guardrail-safe Vega-Lite subset** (inline data,
declarative mark + encoding, no `url`/`expr`), so peacock **compiles that
subset to SVG itself** (`peacock_rasterizer::vegalite_svg`) and rasterizes
SVG → PNG with **`resvg`/`usvg`/`tiny-skia`** — pure Rust, stable, fast, no
Node, no Deno, no network (NFR-S-5). A permissively-licensed font
(`assets/DejaVuSans.ttf`) is vendored for deterministic, offline text. The
crate is a normal main-workspace member again (no `aes` conflict, no sidecar
process boundary needed).

**Scope of the renderer.** The safe subset peacock authors: marks `line` /
`bar` / `point` / `area`; encodings `x` (temporal/ordinal/nominal),
`y` (quantitative, optional `sum` aggregate), `color` (nominal series);
axes with ticks + labels, gridlines, and a legend. This is exactly what
peacock's composer emits — anything outside it is already rejected by the
guardrail (FR-V-4), so we never need full Vega-Lite.

**Recognise it next time.** Any crate bundling Deno/V8 (`deno_core`,
`vl-convert`, `deno_runtime`) drags a brittle, exact-pinned constellation.
Don't co-resolve it with a large unrelated tree, and prefer a pure-Rust path
for a bounded, well-specified subset.

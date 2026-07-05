# ggplot-rs 0.9.2: which geoms actually honour a color/fill grouping

**Symptom.** A stat spec with `color: supplier` renders fine for
`geom: density` but would silently produce a WRONG chart for
`histogram`, `ecdf` (via `geom_step` + `StatEcdf`) and `boxplot` if the
dialect passed the aesthetic through: one merged series, or one path
that interleaves all groups' points sorted by x.

**Cause (verified in the 0.9.2 sources).** Grouped *stat computation* is
generic — `build.rs` groups by `color`/`fill`/`group` + facet vars and
runs the stat per group — but the *draw* side is per-geom:

- `StatBin` (histogram) does not carry the grouping columns into its
  output, and `GeomHistogram::draw` paints one `self.fill` — no
  per-group fill mapping.
- `GeomStep::draw` sorts ALL rows by x and draws a single path with the
  first row's color — multi-group ECDF steps interleave into one
  zigzag.
- `GeomBoxplot::draw` uses one `self.fill` for every box (per-category
  boxes come from grouping by discrete x, which works).
- `GeomDensity::draw` and `GeomLine::draw` DO split per color/fill
  group and map each through the color scale — these are safe.

Facet grouping is safe everywhere: the facet value is re-attached to
each group's stat output in `build.rs` and the renderer filters rows per
panel before calling `draw`.

**Fix in peacock.** `peacock-ggplot` enforces a support matrix (crate
docs of `crates/peacock-ggplot/src/lib.rs`): `color` is accepted for
`density` only; on the other geoms it is a structured render error that
points the author at `facet_wrap`. Never silently drop a declared
aesthetic. Annotation labels on `boxplot` are likewise rejected —
`Annotation::Text` anchors x as `Value::Float` through the scale, which
has no meaning on a discrete category axis.

**How to recognise it next time.** Before mapping a new dialect
aesthetic onto a ggplot-rs geom, read that geom's `draw()` — if it
doesn't split rows by `to_group_key()` over the color/fill column, the
aesthetic will merge series instead of erroring. Also check the geom's
default stat `compute_group`: if it doesn't copy `color`/`fill`/`group`
into its output (as `StatEcdf`/`StatDensity` do), the mapping is lost
before draw.

//! Deterministic brand extraction: a logo PNG → a colour palette → brand CSS.
//!
//! This is the productive, *deterministic* half of AI-assisted styling. peacock
//! itself runs no model (BRD non-goal); an authoring agent (or a styling tool)
//! supplies a corporate logo, peacock extracts the dominant brand colours here,
//! and the agent registers the resulting `:root { --pk-* }` brand CSS. The
//! agent's job is judgement (which host, refining tokens); peacock's job is the
//! deterministic extraction + the deterministic render.

use crate::RasterError;

/// A quantized colour bucket key (4-bit-per-channel grid).
type BucketKey = (u8, u8, u8);
/// Accumulated `(sum_r, sum_g, sum_b, count)` for a bucket.
type BucketAcc = (u64, u64, u64, u64);

/// Extract up to `n` dominant, saturated colours from a logo PNG, most-frequent
/// first. Near-transparent and near-neutral (white/black/grey) pixels are
/// ignored so the result is the *brand* colours, not the background.
pub fn palette_from_png(png: &[u8], n: usize) -> Result<Vec<String>, RasterError> {
    let pixmap = tiny_skia::Pixmap::decode_png(png)
        .map_err(|e| RasterError::new(format!("decode png: {e}")))?;

    // Quantize to a 4-bit-per-channel grid and accumulate average colour +
    // count per bucket — a cheap, deterministic dominant-colour pass.
    use std::collections::HashMap;
    let mut buckets: HashMap<BucketKey, BucketAcc> = HashMap::new();
    for px in pixmap.pixels() {
        let c = px.demultiply();
        let (r, g, b, a) = (c.red(), c.green(), c.blue(), c.alpha());
        if a < 128 {
            continue; // transparent
        }
        let chroma = r.max(g).max(b) - r.min(g).min(b);
        if chroma < 28 {
            continue; // near-neutral (grey/white/black)
        }
        let key = (r >> 4, g >> 4, b >> 4);
        let e = buckets.entry(key).or_default();
        e.0 += r as u64;
        e.1 += g as u64;
        e.2 += b as u64;
        e.3 += 1;
    }

    let mut ranked: Vec<(BucketKey, BucketAcc)> = buckets.into_iter().collect();
    // Most frequent first; ties broken by bucket key for determinism.
    ranked.sort_by(|a, b| b.1.3.cmp(&a.1.3).then(a.0.cmp(&b.0)));

    let out = ranked
        .into_iter()
        .take(n)
        .map(|(_, (sr, sg, sb, cnt))| {
            let r = (sr / cnt) as u8;
            let g = (sg / cnt) as u8;
            let b = (sb / cnt) as u8;
            format!("#{r:02x}{g:02x}{b:02x}")
        })
        .collect::<Vec<_>>();

    if out.is_empty() {
        return Err(RasterError::new(
            "no saturated colours found in logo (all neutral/transparent)",
        ));
    }
    Ok(out)
}

/// Build a brand `:root { --pk-* }` CSS block from a logo PNG — ready to
/// `ThemeRegistry::register_brand` and compose under any host flavour.
pub fn brand_css_from_logo(name: &str, png: &[u8]) -> Result<String, RasterError> {
    let palette = palette_from_png(png, 6)?;
    let brand = &palette[0];
    let accent = palette.get(1).unwrap_or(brand);
    let mut css = String::new();
    css.push_str(":root {\n");
    css.push_str(&format!("  --pk-name: \"{}\";\n", name.replace('"', "'")));
    css.push_str(&format!("  --pk-brand: {brand};\n"));
    css.push_str(&format!("  --pk-accent: {accent};\n"));
    for (i, c) in palette.iter().enumerate() {
        css.push_str(&format!("  --pk-cat-{}: {c};\n", i + 1));
    }
    css.push_str("}\n");
    Ok(css)
}

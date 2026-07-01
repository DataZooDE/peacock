//! Scales: continuous (linear/log/sqrt/pow/time) and discrete (band/point),
//! plus categorical + sequential colour schemes.

use serde_json::Value;

/// A continuous numeric scale mapping a data value to a pixel position within
/// `[range0, range1]`.
#[derive(Clone, Debug)]
pub struct LinearScale {
    pub domain_min: f64,
    pub domain_max: f64,
    pub range0: f64,
    pub range1: f64,
    pub kind: ContinuousKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContinuousKind {
    Linear,
    Log,
    Sqrt,
    Pow,
}

impl LinearScale {
    pub fn new(domain_min: f64, domain_max: f64, range0: f64, range1: f64) -> Self {
        Self {
            domain_min,
            domain_max,
            range0,
            range1,
            kind: ContinuousKind::Linear,
        }
    }

    pub fn with_kind(mut self, kind: ContinuousKind) -> Self {
        self.kind = kind;
        self
    }

    fn transform(&self, v: f64) -> f64 {
        match self.kind {
            ContinuousKind::Linear => v,
            ContinuousKind::Log => {
                // Guard against non-positive values.
                if v <= 0.0 { f64::NEG_INFINITY } else { v.ln() }
            }
            ContinuousKind::Sqrt => v.max(0.0).sqrt(),
            ContinuousKind::Pow => v.signum() * v.abs().powf(2.0),
        }
    }

    /// Map a data value to a pixel position.
    pub fn map(&self, v: f64) -> f64 {
        let d0 = self.transform(self.domain_min);
        let d1 = self.transform(self.domain_max);
        let tv = self.transform(v);
        if (d1 - d0).abs() < f64::EPSILON {
            return (self.range0 + self.range1) / 2.0;
        }
        let t = (tv - d0) / (d1 - d0);
        self.range0 + t * (self.range1 - self.range0)
    }

    /// Tick values across the domain (count is a hint).
    pub fn ticks(&self, count: usize) -> Vec<f64> {
        if self.kind == ContinuousKind::Log {
            return log_ticks(self.domain_min, self.domain_max);
        }
        let n = count.max(1);
        (0..=n)
            .map(|i| {
                self.domain_min + (self.domain_max - self.domain_min) * (i as f64) / (n as f64)
            })
            .collect()
    }
}

fn log_ticks(min: f64, max: f64) -> Vec<f64> {
    if min <= 0.0 || max <= 0.0 {
        return vec![min.max(1.0), max.max(10.0)];
    }
    let lo = min.log10().floor() as i32;
    let hi = max.log10().ceil() as i32;
    let mut out = Vec::new();
    for e in lo..=hi {
        let v = 10f64.powi(e);
        if v >= min * 0.999 && v <= max * 1.001 {
            out.push(v);
        }
    }
    if out.len() < 2 {
        out = vec![min, max];
    }
    out
}

/// A discrete band/point scale for ordinal/nominal positions.
#[derive(Clone, Debug)]
pub struct BandScale {
    pub n: usize,
    pub range0: f64,
    pub range1: f64,
    pub padding: f64,
    pub point: bool,
}

impl BandScale {
    pub fn band(n: usize, range0: f64, range1: f64, padding: f64) -> Self {
        Self {
            n,
            range0,
            range1,
            padding,
            point: false,
        }
    }

    pub fn point(n: usize, range0: f64, range1: f64) -> Self {
        Self {
            n,
            range0,
            range1,
            padding: 0.0,
            point: true,
        }
    }

    pub fn step(&self) -> f64 {
        if self.n == 0 {
            return self.range1 - self.range0;
        }
        (self.range1 - self.range0) / (self.n as f64)
    }

    pub fn bandwidth(&self) -> f64 {
        if self.point {
            return 0.0;
        }
        self.step() * (1.0 - self.padding)
    }

    /// Left edge of band `i` (for bars/rects).
    pub fn band_start(&self, i: usize) -> f64 {
        let step = self.step();
        self.range0 + step * (i as f64) + step * self.padding / 2.0
    }

    /// Centre of band/point `i` (for points/lines/labels).
    pub fn center(&self, i: usize) -> f64 {
        if self.point {
            if self.n <= 1 {
                return (self.range0 + self.range1) / 2.0;
            }
            self.range0 + (self.range1 - self.range0) * (i as f64) / ((self.n - 1) as f64)
        } else {
            self.band_start(i) + self.bandwidth() / 2.0
        }
    }
}

/// Round a max up to a "nice" axis ceiling (1/2/2.5/5 × 10^k).
pub fn nice_ceiling(max: f64) -> f64 {
    if max <= 0.0 {
        return 1.0;
    }
    let mag = 10f64.powf(max.log10().floor());
    let norm = max / mag;
    let nice = if norm <= 1.0 {
        1.0
    } else if norm <= 2.0 {
        2.0
    } else if norm <= 2.5 {
        2.5
    } else if norm <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * mag
}

/// Round a min *down* to a nice floor (mirror of [`nice_ceiling`]).
pub fn nice_floor(min: f64) -> f64 {
    if min >= 0.0 {
        return 0.0;
    }
    -nice_ceiling(-min)
}

// ---------------------------------------------------------------------------
// Colour schemes
// ---------------------------------------------------------------------------

/// Vega's default categorical palette (Tableau10).
pub const TABLEAU10: &[&str] = &[
    "#4c78a8", "#f58518", "#54a24b", "#e45756", "#72b7b2", "#ff9da6", "#9d755d", "#bab0ac",
    "#e377c2", "#17becf",
];

/// D3 `category10`.
pub const CATEGORY10: &[&str] = &[
    "#1f77b4", "#ff7f0e", "#2ca02c", "#d62728", "#9467bd", "#8c564b", "#e377c2", "#7f7f7f",
    "#bcbd22", "#17becf",
];

/// Resolve a named categorical scheme to its palette (defaults to Tableau10).
pub fn categorical_scheme(name: Option<&str>) -> &'static [&'static str] {
    match name {
        Some("category10") => CATEGORY10,
        Some("tableau10") | None => TABLEAU10,
        _ => TABLEAU10,
    }
}

// Canonical 9-stop ramps (matplotlib viridis/magma, ColorBrewer Blues/Greens)
// — linearly interpolated by `sequential_color`, so more stops ⇒ truer colour.
const VIRIDIS: &[(u8, u8, u8)] = &[
    (68, 1, 84),
    (71, 45, 123),
    (59, 82, 139),
    (44, 114, 142),
    (33, 145, 140),
    (40, 174, 128),
    (94, 201, 98),
    (173, 220, 48),
    (253, 231, 37),
];
const MAGMA: &[(u8, u8, u8)] = &[
    (0, 0, 4),
    (28, 16, 68),
    (79, 18, 123),
    (129, 37, 129),
    (181, 54, 122),
    (229, 80, 100),
    (251, 135, 97),
    (254, 194, 135),
    (252, 253, 191),
];
const BLUES: &[(u8, u8, u8)] = &[
    (247, 251, 255),
    (222, 235, 247),
    (198, 219, 239),
    (158, 202, 225),
    (107, 174, 214),
    (66, 146, 198),
    (33, 113, 181),
    (8, 81, 156),
    (8, 48, 107),
];
const GREENS: &[(u8, u8, u8)] = &[
    (247, 252, 245),
    (229, 245, 224),
    (199, 233, 192),
    (161, 217, 155),
    (116, 196, 118),
    (65, 171, 93),
    (35, 139, 69),
    (0, 109, 44),
    (0, 68, 27),
];
// Green → amber → red diverging ramp (ColorBrewer RdYlGn, reversed): low value
// = healthy green, high value = alarming red. For risk-style colouring where a
// higher score is worse.
const RISK: &[(u8, u8, u8)] = &[
    (26, 152, 80),
    (102, 189, 99),
    (166, 217, 106),
    (217, 239, 139),
    (254, 224, 139),
    (253, 174, 97),
    (244, 109, 67),
    (215, 48, 39),
    (165, 0, 38),
];

thread_local! {
    /// A brand-derived sequential ramp set by the themed render path; when
    /// present it overrides the named scheme so heatmaps/continuous colour pick
    /// up the corporate hue. Thread-local because a render is synchronous on one
    /// thread, and it keeps the theme out of every function signature.
    static SEQ_OVERRIDE: std::cell::RefCell<Option<Vec<(u8, u8, u8)>>> =
        const { std::cell::RefCell::new(None) };
}

/// Set (or clear) the brand sequential ramp for the current thread.
pub fn set_sequential_override(ramp: Option<Vec<(u8, u8, u8)>>) {
    SEQ_OVERRIDE.with(|s| *s.borrow_mut() = ramp);
}

/// Interpolate a sequential colour at `t` in `[0,1]`: a brand override if set,
/// else the named scheme.
pub fn sequential_color(scheme: Option<&str>, t: f64) -> String {
    let override_ramp = SEQ_OVERRIDE.with(|s| s.borrow().clone());
    let named: &[(u8, u8, u8)] = match scheme {
        Some("blues") => BLUES,
        Some("greens") => GREENS,
        // Risk-style green→red (low = green, high = red).
        Some("risk") | Some("redyellowgreen") | Some("greenred") => RISK,
        Some("magma") | Some("inferno") | Some("plasma") => MAGMA,
        _ => VIRIDIS,
    };
    let ramp: &[(u8, u8, u8)] = override_ramp.as_deref().unwrap_or(named);
    if ramp.len() < 2 {
        let (r, g, b) = ramp.first().copied().unwrap_or((0, 0, 0));
        return format!("#{r:02x}{g:02x}{b:02x}");
    }
    let t = t.clamp(0.0, 1.0);
    let seg = (ramp.len() - 1) as f64;
    let pos = t * seg;
    let i = (pos.floor() as usize).min(ramp.len() - 2);
    let f = pos - i as f64;
    let (r0, g0, b0) = ramp[i];
    let (r1, g1, b1) = ramp[i + 1];
    let lerp = |a: u8, b: u8| -> u8 { (a as f64 + (b as f64 - a as f64) * f).round() as u8 };
    format!(
        "#{:02x}{:02x}{:02x}",
        lerp(r0, r1),
        lerp(g0, g1),
        lerp(b0, b1)
    )
}

/// Pull a `scale.scheme` name (lower-cased) out of an encoding channel def.
pub fn scheme_name(channel: &Value) -> Option<String> {
    channel
        .get("scale")
        .and_then(|s| s.get("scheme"))
        .and_then(Value::as_str)
        .map(|s| s.to_ascii_lowercase())
}

/// Pull an explicit `scale.range` (array of colour strings).
pub fn explicit_color_range(channel: &Value) -> Option<Vec<String>> {
    channel
        .get("scale")
        .and_then(|s| s.get("range"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
}

#[cfg(test)]
mod tests {
    use super::sequential_color;

    #[test]
    fn sequential_schemes_resolve_and_vary() {
        // Endpoints differ from the midpoint, and the named schemes differ.
        for s in [
            Some("viridis"),
            Some("magma"),
            Some("blues"),
            Some("greens"),
            None,
        ] {
            let lo = sequential_color(s, 0.0);
            let mid = sequential_color(s, 0.5);
            let hi = sequential_color(s, 1.0);
            assert_ne!(lo, hi, "{s:?} ramp endpoints differ");
            assert_ne!(lo, mid, "{s:?} ramp interpolates");
        }
        assert_ne!(
            sequential_color(Some("blues"), 0.5),
            sequential_color(Some("greens"), 0.5)
        );
    }

    #[test]
    fn risk_scheme_runs_green_low_to_red_high() {
        let hex = |s: &str| {
            let h = s.trim_start_matches('#');
            (
                u8::from_str_radix(&h[0..2], 16).unwrap(),
                u8::from_str_radix(&h[2..4], 16).unwrap(),
                u8::from_str_radix(&h[4..6], 16).unwrap(),
            )
        };
        // Low value = green (green channel leads), high value = red (red leads).
        let (lr, lg, _) = hex(&sequential_color(Some("risk"), 0.0));
        let (hr, hg, _) = hex(&sequential_color(Some("risk"), 1.0));
        assert!(lg > lr, "low risk is green");
        assert!(hr > hg, "high risk is red");
        // Aliases resolve to the same ramp.
        assert_eq!(
            sequential_color(Some("greenred"), 0.3),
            sequential_color(Some("risk"), 0.3)
        );
        assert_eq!(
            sequential_color(Some("redyellowgreen"), 0.7),
            sequential_color(Some("risk"), 0.7)
        );
    }
}

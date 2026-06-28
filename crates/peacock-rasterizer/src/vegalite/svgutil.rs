//! Small SVG-emission helpers shared across the renderer.

/// XML-escape text content / attribute values.
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Trim an ISO date like `1997-01-01` to `1997-01` for a compact axis label.
pub fn short_label(s: &str) -> String {
    if s.len() == 10 && s.as_bytes().get(4) == Some(&b'-') && s.as_bytes().get(7) == Some(&b'-') {
        s[..7].to_owned()
    } else {
        s.to_owned()
    }
}

/// Format a number with a (very small) subset of d3-format specifiers used in
/// Vega-Lite axes: `%` (percent), `$` prefix, `.<n>f` fixed decimals, `,`
/// thousands grouping. Anything unrecognised falls back to [`fmt_num`].
pub fn fmt_with(v: f64, spec: &str) -> String {
    let percent = spec.contains('%');
    let dollar = spec.contains('$');
    let group = spec.contains(',');
    let mut x = v;
    if percent {
        x *= 100.0;
    }
    // fixed decimals: ".Nf"
    let decimals = spec.split('.').nth(1).and_then(|s| {
        s.chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<usize>()
            .ok()
    });
    let mut body = match decimals {
        Some(d) => format!("{x:.*}", d),
        None => fmt_num(x),
    };
    if group {
        body = group_thousands(&body);
    }
    let mut out = String::new();
    if dollar {
        out.push('$');
    }
    out.push_str(&body);
    if percent {
        out.push('%');
    }
    out
}

fn group_thousands(s: &str) -> String {
    let neg = s.starts_with('-');
    let s2 = s.trim_start_matches('-');
    let (int, frac) = match s2.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (s2, None),
    };
    let mut grouped = String::new();
    let bytes: Vec<char> = int.chars().collect();
    for (i, c) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(*c);
    }
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    out.push_str(&grouped);
    if let Some(f) = frac {
        out.push('.');
        out.push_str(f);
    }
    out
}

/// Format a number for an axis tick / label without trailing noise.
pub fn fmt_num(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_owned();
    }
    if (v - v.round()).abs() < 1e-9 {
        return format!("{}", v.round() as i64);
    }
    let a = v.abs();
    if !(0.01..1000.0).contains(&a) {
        // Compact for very large / small numbers.
        format!("{v:.2}")
    } else {
        // strip trailing zeros
        format!("{v:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    }
}

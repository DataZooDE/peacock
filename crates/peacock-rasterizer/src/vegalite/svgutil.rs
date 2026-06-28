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

/// Format a number with a practical subset of d3-format specifiers used in
/// Vega-Lite axes/labels:
/// `[+][$][,][.precision][type]` where `type` ∈ `f` (fixed) · `%` (percent) ·
/// `s` (SI prefix: k/M/G/T, m/µ/n) · `e` (exponent); `+` forces a sign, `$` a
/// currency symbol, `,` thousands grouping. Unrecognised input falls back to
/// [`fmt_num`].
pub fn fmt_with(v: f64, spec: &str) -> String {
    let sign_plus = spec.contains('+');
    let dollar = spec.contains('$');
    let group = spec.contains(',');
    let precision = spec.split('.').nth(1).and_then(|s| {
        s.chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<usize>()
            .ok()
    });
    // The conversion type is the last of these specifier letters present.
    let ty = spec
        .chars()
        .rev()
        .find(|c| matches!(c, 'f' | '%' | 's' | 'e'));

    let neg = v.is_sign_negative() && v != 0.0;
    let mag = v.abs();

    let mut body = match ty {
        Some('%') => format!("{:.*}%", precision.unwrap_or(0), mag * 100.0),
        Some('e') => format!("{:.*e}", precision.unwrap_or(2), mag),
        Some('s') => si_format(mag, precision),
        Some('f') => format!("{:.*}", precision.unwrap_or(0), mag),
        _ => match precision {
            Some(p) => format!("{mag:.p$}"),
            None => fmt_num(mag),
        },
    };
    // Group only plain numeric bodies (an SI suffix like `k` is not a digit).
    if group && ty != Some('s') {
        body = group_thousands(&body);
    }

    let mut out = String::new();
    if neg {
        out.push('-');
    } else if sign_plus {
        out.push('+');
    }
    if dollar {
        out.push('$');
    }
    out.push_str(&body);
    out
}

/// SI-prefix formatting (`s` type): `1500 → 1.5k`, `2.5e6 → 2.5M`, `0.002 → 2m`.
fn si_format(mag: f64, precision: Option<usize>) -> String {
    let p = precision.unwrap_or(1);
    let (scaled, suffix) = if mag == 0.0 {
        (0.0, "")
    } else if mag >= 1e12 {
        (mag / 1e12, "T")
    } else if mag >= 1e9 {
        (mag / 1e9, "G")
    } else if mag >= 1e6 {
        (mag / 1e6, "M")
    } else if mag >= 1e3 {
        (mag / 1e3, "k")
    } else if mag >= 1.0 {
        (mag, "")
    } else if mag >= 1e-3 {
        (mag * 1e3, "m")
    } else if mag >= 1e-6 {
        (mag * 1e6, "µ")
    } else {
        (mag * 1e9, "n")
    };
    let s = format!("{scaled:.p$}");
    // Trim trailing zeros so `1.50k` reads `1.5k` (d3 drops insignificant zeros).
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        s
    };
    format!("{s}{suffix}")
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

#[cfg(test)]
mod tests {
    use super::fmt_with;

    #[test]
    fn d3_format_subset() {
        assert_eq!(fmt_with(0.5, "%"), "50%");
        assert_eq!(fmt_with(0.123, ".1%"), "12.3%");
        assert_eq!(fmt_with(1234.0, ","), "1,234");
        assert_eq!(fmt_with(1234.5, "$,.2f"), "$1,234.50");
        assert_eq!(fmt_with(1500.0, "s"), "1.5k");
        assert_eq!(fmt_with(2_500_000.0, ".1s"), "2.5M");
        assert_eq!(fmt_with(0.002, "s"), "2m");
        assert_eq!(fmt_with(42.0, "+"), "+42");
        assert_eq!(fmt_with(-42.0, "f"), "-42");
        assert_eq!(fmt_with(1234.0, "$,"), "$1,234");
        assert!(fmt_with(1200.0, "e").contains('e'));
    }
}

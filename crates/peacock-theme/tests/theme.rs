//! Theme parsing, composition, and (brand × host) resolution.

use peacock_theme::{ThemeRegistry, ThemeTokens, compose, parse_vars};

#[test]
fn parses_pk_custom_properties_last_wins() {
    let css = r#"
        /* comment */
        :root {
            --pk-brand: #111111;
            --pk-cat-1: #aaa; --pk-cat-2: #bbb;
            --pk-radius: 8px;
        }
        :root { --pk-brand: #222222; }  /* later wins */
    "#;
    let v = parse_vars(css);
    assert_eq!(v.get("brand").unwrap(), "#222222");
    assert_eq!(v.get("cat-1").unwrap(), "#aaa");
    assert_eq!(v.get("radius").unwrap(), "8px");

    let t = ThemeTokens::from_vars(&v);
    assert_eq!(t.brand, "#222222");
    assert_eq!(t.radius, 8.0);
    assert_eq!(t.palette, vec!["#aaa", "#bbb"]);
}

#[test]
fn defaults_when_no_theme() {
    let t = ThemeTokens::default();
    assert_eq!(t.brand, "#0f6cbd");
    assert!(t.palette.len() >= 6);
    assert!(t.logo.is_none());
}

#[test]
fn compose_layers_brand_over_host() {
    let host = ":root { --pk-bg: #efeae2; --pk-brand: #075e54; }";
    let brand = ":root { --pk-brand: #6b3fa0; --pk-name: \"Acme A\"; }";
    let css = compose(host, brand);
    let t = ThemeTokens::from_vars(&parse_vars(&css));
    // Brand overrides the brand colour…
    assert_eq!(t.brand, "#6b3fa0");
    // …but inherits the host's background (corporate identity ⊕ host look).
    assert_eq!(t.bg, "#efeae2");
    assert_eq!(t.name.as_deref(), Some("Acme A"));
}

#[test]
fn registry_resolves_brand_by_host_distinctly() {
    let reg = ThemeRegistry::builtin();

    let a_whatsapp = reg.resolve("company-a", "whatsapp");
    let a_copilot = reg.resolve("company-a", "copilot");
    let b_gemini = reg.resolve("company-b", "gemini");

    // Same brand, different host → same brand colour, different background.
    assert_eq!(a_whatsapp.tokens.brand, "#6b3fa0");
    assert_eq!(a_copilot.tokens.brand, "#6b3fa0");
    assert_ne!(a_whatsapp.tokens.bg, a_copilot.tokens.bg);

    // Different brand → different brand colour + palette.
    assert_eq!(b_gemini.tokens.brand, "#e8590c");
    assert_ne!(a_copilot.tokens.palette, b_gemini.tokens.palette);

    // The composed CSS is what web surfaces consume.
    assert!(a_whatsapp.css.contains("--pk-brand"));
}

#[test]
fn agent_authored_brand_can_be_registered() {
    let mut reg = ThemeRegistry::builtin();
    reg.register_brand(
        "startup-z",
        ":root { --pk-brand: #00b894; --pk-name: \"Z\"; }",
    );
    let t = reg.resolve("startup-z", "copilot");
    assert_eq!(t.tokens.brand, "#00b894");
    assert_eq!(t.tokens.name.as_deref(), Some("Z"));
    // Inherits Copilot's surface.
    assert_eq!(t.tokens.surface, "#f3f2f1");
}

#[test]
fn material_host_flavor_is_google_blue() {
    // The `material` flavor styles the standalone document view (the agent's
    // /docs page + the Gemini Enterprise side panel). Its brand deliberately
    // equals Triton's GE THEME_PRIMARY_COLOR so the panel matches the card.
    let reg = ThemeRegistry::builtin();
    let t = reg.resolve("default", "material");
    assert_eq!(t.tokens.brand, "#1a73e8");
    assert_eq!(t.tokens.accent, "#1a73e8");
    assert!(t.css.contains("--pk-brand"));
    // A tenant brand still composes on top of the Material host look.
    let branded = reg.resolve("company-a", "material");
    assert_eq!(branded.tokens.brand, "#6b3fa0");
}

#[test]
fn unknown_names_fall_back_to_defaults() {
    let reg = ThemeRegistry::builtin();
    let t = reg.resolve("nope", "nope");
    assert_eq!(t.tokens, ThemeTokens::default());
}

#[test]
fn logo_style_parses_and_defaults_to_avatar() {
    use peacock_theme::LogoStyle;

    // Absent → avatar (the small round header image).
    let t = peacock_theme::ThemeTokens::default();
    assert_eq!(t.logo_style, LogoStyle::Avatar);

    // `--pk-logo-style: banner` → banner (full-width wordmark).
    let vars = peacock_theme::parse_vars(
        ":root { --pk-logo: https://brand.example/logo.png; --pk-logo-style: banner; }",
    );
    let t = peacock_theme::ThemeTokens::from_vars(&vars);
    assert_eq!(t.logo_style, LogoStyle::Banner);
    assert_eq!(t.logo.as_deref(), Some("https://brand.example/logo.png"));

    // Junk value → default, never an error (theming never fails a request).
    let vars = peacock_theme::parse_vars(":root { --pk-logo-style: sideways; }");
    let t = peacock_theme::ThemeTokens::from_vars(&vars);
    assert_eq!(t.logo_style, LogoStyle::Avatar);

    // Wire names for the chrome consumers.
    assert_eq!(LogoStyle::Avatar.as_str(), "avatar");
    assert_eq!(LogoStyle::Banner.as_str(), "banner");
}

// ── anofox theme bridge (issue #44) ─────────────────────────────────────────

use peacock_theme::{Variant, contrast_ratio, lift_to_contrast};

fn acme() -> ThemeTokens {
    ThemeTokens {
        font: "Inter, sans-serif".into(),
        brand: "#6b3fa0".into(),
        accent: "#ff8800".into(),
        bg: "#fafafa".into(),
        surface: "#ffffff".into(),
        text: "#111111".into(),
        muted: "#666666".into(),
        border: "#dddddd".into(),
        radius: 6.0,
        palette: vec!["#6b3fa0".into(), "#ff8800".into(), "#00aa77".into()],
        name: Some("Acme A".into()),
        ..Default::default()
    }
}

#[test]
fn anofox_json_maps_every_token() {
    let j = acme().to_anofox_json(Variant::Light);
    assert_eq!(
        j["palette"],
        serde_json::json!(["#6b3fa0", "#ff8800", "#00aa77"])
    );
    let c = &j["colors"];
    assert_eq!(c["accent"], "#6b3fa0");
    assert_eq!(c["info"], "#6b3fa0");
    assert_eq!(c["link"], "#6b3fa0");
    assert_eq!(c["accent2"], "#ff8800");
    assert_eq!(c["bg"], "#fafafa");
    assert_eq!(c["card"], "#ffffff");
    assert_eq!(c["ink"], "#111111");
    assert_eq!(c["heading"], "#111111");
    assert_eq!(c["muted"], "#666666");
    assert_eq!(c["line"], "#dddddd");
    assert_eq!(j["font"]["family"], "Inter, sans-serif");
    assert_eq!(j["radius"], 6.0);
    assert_eq!(j["brand"]["name"], "Acme A");
    assert_eq!(
        j["sequential"][1], "#6b3fa0",
        "sequential high end is the brand"
    );
    assert_eq!(j["mode"], "light");
}

#[test]
fn anofox_json_is_resolvable_by_anofox() {
    // The JSON must be a theme anofox accepts (no unknown/invalid keys).
    for v in [Variant::Light, Variant::Dark] {
        let j = acme().to_anofox_json(v);
        anofox_visualization::theme::resolve_json(&j)
            .unwrap_or_else(|e| panic!("{v:?} theme rejected by anofox: {e}"));
    }
}

#[test]
fn contrast_ratio_matches_wcag() {
    assert!((contrast_ratio("#000000", "#ffffff").unwrap() - 21.0).abs() < 0.01);
    assert!((contrast_ratio("#ffffff", "#ffffff").unwrap() - 1.0).abs() < 0.01);
}

#[test]
fn dark_variant_lifts_accent_to_aa() {
    let j = acme().to_anofox_json(Variant::Dark);
    let card = j["dark"]["colors"]["card"].as_str().expect("dark card");
    let accent = j["dark"]["colors"]["accent"].as_str().expect("dark accent");
    let ratio = contrast_ratio(accent, card).unwrap();
    assert!(
        ratio >= 4.5,
        "dark accent {accent} on {card} = {ratio:.2}:1"
    );
    assert_ne!(
        accent, "#6b3fa0",
        "the brand purple fails on dark and must be lifted"
    );
    assert_eq!(j["mode"], "dark");
}

#[test]
fn lift_preserves_a_compliant_colour_and_keeps_hue() {
    assert_eq!(lift_to_contrast("#ffffff", "#000000", 4.5), "#ffffff");
    let lifted = lift_to_contrast("#6b3fa0", "#162033", 4.5);
    assert!(contrast_ratio(&lifted, "#162033").unwrap() >= 4.5);
    // Still a purple: red and blue dominate green.
    let (r, g, b) = (
        u8::from_str_radix(&lifted[1..3], 16).unwrap(),
        u8::from_str_radix(&lifted[3..5], 16).unwrap(),
        u8::from_str_radix(&lifted[5..7], 16).unwrap(),
    );
    assert!(b > g && r > g, "hue preserved, got {lifted}");
}

#[test]
fn light_variant_leaves_a_compliant_brand_untouched() {
    let j = ThemeTokens::default().to_anofox_json(Variant::Light);
    assert_eq!(j["colors"]["accent"], "#0f6cbd");
}

#[test]
fn dark_palette_stays_distinct_after_lifting() {
    // Lifting several dark purples to the same contrast floor must not
    // collapse two series onto one colour.
    let t = ThemeRegistry::builtin()
        .resolve("company-a", "copilot")
        .tokens;
    let j = t.to_anofox_json(Variant::Dark);
    let pal: Vec<&str> = j["dark"]["palette"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let rgb = |h: &str| -> [f64; 3] {
        let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap() as f64;
        [p(1), p(3), p(5)]
    };
    for (i, a) in pal.iter().enumerate() {
        for b in &pal[i + 1..] {
            let (x, y) = (rgb(a), rgb(b));
            let d = ((x[0] - y[0]).powi(2) + (x[1] - y[1]).powi(2) + (x[2] - y[2]).powi(2)).sqrt();
            assert!(
                d >= 24.0,
                "dark palette {a} vs {b} too close ({d:.1}): {pal:?}"
            );
        }
        assert!(contrast_ratio(a, peacock_theme::DARK_CARD).unwrap() >= 3.0);
    }
}

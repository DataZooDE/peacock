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

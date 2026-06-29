//! Logo → brand-palette extraction, and the agent loop: extracted brand CSS
//! registered and resolved under a host.

use peacock_rasterizer::{ThemeRegistry, brand_css_from_logo, palette_from_png};

/// A tiny "logo": left half brand-red, right half brand-blue, on a white
/// (neutral, ignored) margin.
fn fake_logo() -> Vec<u8> {
    let mut pm = tiny_skia::Pixmap::new(40, 20).unwrap();
    pm.fill(tiny_skia::Color::WHITE);
    let red = tiny_skia::Color::from_rgba8(0xd6, 0x33, 0x2a, 0xff);
    let blue = tiny_skia::Color::from_rgba8(0x1c, 0x6f, 0xd6, 0xff);
    let mut paint = tiny_skia::Paint::default();
    paint.set_color(red);
    pm.fill_rect(
        tiny_skia::Rect::from_xywh(0.0, 0.0, 18.0, 20.0).unwrap(),
        &paint,
        tiny_skia::Transform::identity(),
        None,
    );
    paint.set_color(blue);
    pm.fill_rect(
        tiny_skia::Rect::from_xywh(22.0, 0.0, 18.0, 20.0).unwrap(),
        &paint,
        tiny_skia::Transform::identity(),
        None,
    );
    pm.encode_png().unwrap()
}

#[test]
fn extracts_dominant_brand_colours_ignoring_neutrals() {
    let palette = palette_from_png(&fake_logo(), 4).unwrap();
    // The white margin is ignored; the two brand colours come out.
    assert!(palette.len() >= 2, "got {palette:?}");
    let joined = palette.join(",");
    assert!(
        joined.contains("#d") || joined.contains("#c"),
        "red-ish present: {joined}"
    );
    assert!(
        palette
            .iter()
            .any(|c| c.starts_with("#1") || c.starts_with("#2")),
        "blue-ish present: {palette:?}"
    );
    // No near-white.
    assert!(!palette.iter().any(|c| c == "#ffffff"), "neutrals excluded");
}

#[test]
fn agent_loop_logo_to_registered_brand_theme() {
    // The productive loop: logo → brand CSS → register → resolve under a host.
    let css = brand_css_from_logo("Logo Co", &fake_logo()).unwrap();
    assert!(css.contains("--pk-brand"));
    assert!(css.contains("--pk-name: \"Logo Co\""));

    let mut reg = ThemeRegistry::builtin();
    reg.register_brand("logo-co", css);
    let theme = reg.resolve("logo-co", "gemini");
    // Brand colour came from the logo; host look (Gemini surface) is inherited.
    assert!(theme.tokens.brand.starts_with('#'));
    assert_eq!(theme.tokens.surface, "#f8f9fa");
}

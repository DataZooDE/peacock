# peacock theming — corporate identity ⊕ host look, from one CSS file

## The need

The same report must look **different** per company and per host it's embedded
in: Company A via WhatsApp, Company A via Copilot Studio, and Company B via
Gemini Enterprise should each have their own skin — and styling should be
quick and AI-assisted to set up.

## The model

A theme is a small set of **`--pk-*` CSS custom properties** (design tokens):
series palette, brand/accent colour, fonts, background/surface/text/muted/
border, gridlines, axis colour, corner radius, logo, company name. **One CSS
file is the single source of truth** — web surfaces (the iframe / Flutter /
demo) consume it natively (the cascade does the work), and peacock extracts the
same tokens to style the **server-rendered chart PNG**. The chart and the chrome
around it always match, from one definition.

The look is a **composition of two layers** — the host's look-and-feel under the
company's corporate identity:

```
theme(tenant, host) = host-flavor CSS  ⊕  brand CSS      (brand wins the cascade)

Company A / WhatsApp = whatsapp-flavor ⊕ company-a-brand   (beige chat bg, purple identity)
Company A / Copilot  = copilot-flavor  ⊕ company-a-brand   (light Fluent bg, purple identity)
Company B / Gemini   = gemini-flavor   ⊕ company-b-brand   (white rounded bg, orange identity)
```

Brand CSS is tiny — it overrides only the identity tokens, so it inherits each
host's chrome (background, density, radius). That's why one small brand file
works across every host.

## Where it lives

- **`peacock-theme`** — `ThemeTokens`, a tiny `--pk-*` CSS parser, host⊕brand
  `compose`, and a `ThemeRegistry` (built-in host flavors `copilot` / `whatsapp`
  / `gemini`; brands registerable at runtime). No TOML/JSON — CSS is the format.
- **`peacock-rasterizer`** — `apply_chart_theme` / `apply_dashboard_theme`
  restyle the rendered SVG with the tokens (palette, font, bg, grid, axis,
  text); `render_vega_to_png_themed` / `render_dashboard_to_png_themed`.
- **`peacock-core`** — `RenderOpts.theme` themes the artifact's chart PNG.
- **`peacock-server`** — `AppState.themes`; `render_report` takes `host` +
  `brand` (brand defaults to the caller's tenant), resolves the theme, returns
  the themed PNG **and** `theme_css` (for the web surfaces).

## The AI-assisted authoring loop (peacock stays LLM-free)

peacock runs no model (BRD non-goal). The "AI" is an authoring agent (or a
styling tool); peacock gives it **deterministic** building blocks so the loop is
fast and reproducible:

1. `palette_from_png(logo) → [hex…]` and `brand_css_from_logo(name, logo)` —
   extract a company's brand colours straight from its logo (pure Rust, offline).
2. `ThemeRegistry::register_brand(name, css)` — install the generated identity.
3. `resolve(brand, host)` → a `Theme { css, tokens }` — apply it deterministically.
4. A fast preview: `render_vega_to_png_themed(spec, scale, &tokens)` to iterate.

So an agent can take "here's Acme's logo, target Copilot" → brand CSS →
registered → previewed in seconds, then hand-tune the CSS tokens. The same
applies for matching a host's look (the host flavors are just CSS an agent can
edit or generate).

## Try it

- Gallery: `cargo run -p peacock-rasterizer --example theme_gallery -- <dir>`
  writes the same chart as `company-a/whatsapp`, `company-a/copilot`,
  `company-b/gemini` — visibly different skins.
- Service: `POST /v1/render_report { report_id, host, brand, png:true }` →
  `{ png_base64, theme_css, theme:{host,brand}, … }`.

## Scope / future

Implemented: token model + CSS parse/compose/registry; chart + dashboard
restyling; logo→palette; service resolution by (tenant, host); themed demo
path; built-in host flavors + two demo brands. Not yet: theming the continuous
(sequential) colour scale per brand; vendoring brand fonts for the *chart*
(today the chart falls back to the embedded face — the web surfaces use the real
font); a brand-theme authoring UI. Brand themes are peacock-resident today; they
could later be escurel instances (versioned/ACL'd) if cross-tenant sharing is
wanted.

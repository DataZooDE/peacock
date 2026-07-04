# 04 — Theming: peacock owns ALL of it

**One CSS file is the single source of truth.** A theme is a set of
`--pk-*` custom properties; web surfaces consume the CSS natively
(`__THEME_CSS__` spliced into the iframe), the rasterizer extracts the
same tokens to style chart PNGs and instance cards, and chat adapters
fetch the resolved values for their card chrome via `get_theme`. Chart,
iframe and chat card always match, from one definition. There is **no
theme config anywhere else** — the old Triton manifest `theme:` block
is gone (triton#176).

## The tokens

| Token | Drives |
|---|---|
| `--pk-name` | company display name → card/header **title** |
| `--pk-logo` | logo URL → card header avatar / banner |
| `--pk-logo-style` | `avatar` (round header image) \| `banner` (full-width) |
| `--pk-brand`, `--pk-accent` | primary/secondary colours → KPIs, buttons |
| `--pk-font`, `--pk-bg`, `--pk-surface`, `--pk-text`, `--pk-muted`, `--pk-border`, `--pk-grid`, `--pk-axis`, `--pk-radius` | chrome + chart styling |
| `--pk-cat-1..N` | the categorical series palette |

Every token has a stock default; a brand overrides only what it cares
about. Unrecognised values (e.g. a junk `logo-style`) fall back to the
default — **theming never fails a request**.

## Resolution: host ⊕ brand

`resolve(brand, host)` composes the HOST flavor's CSS (copilot /
whatsapp / gemini built-ins) under the BRAND overlay — plain
concatenation; the brand's declarations come last and win the cascade.
At every server surface, `brand = the deployment principal's tenant`
and `host = the HTTP Host header`. Unknown names → stock look, never an
error.

## Configuring a deployment brand

```bash
PEACOCK_BRAND_CSS=/path/to/brand.css   # registered under PEACOCK_TENANT at boot
```

```css
:root {
  --pk-name: "DataZoo Sales";
  --pk-brand: #0e7a5f;  --pk-accent: #14b58c;
  --pk-logo-style: avatar;
  --pk-cat-1: #0e7a5f;  --pk-cat-2: #14b58c;  /* … */
}
```

An unreadable path is a **named fatal boot error** — a deployment that
configures a brand must actually get it. (Brand CSS is also
agent-authorable: `ThemeRegistry::register_brand(name, css)` is the
embedding hook.)

## `get_theme` — the resolved theme as data

`tools/call get_theme` (no args; tenant + Host derived from the request)
returns:

```json
{ "brand": "acme", "host": "chat.example",
  "title": "DataZoo Sales", "logo_url": null, "logo_style": "avatar",
  "brand_color": "#0e7a5f", "accent": "#14b58c", "css": "…composed css…" }
```

Wired on `/mcp` and the `X-Triton-Tool` path. This is what Triton's
google_chat adapter calls per reply to brand its Cards v2 chrome —
register `get_theme=<peacock>` in `TRITON_STATIC_UPSTREAMS`. Consumers
building their own surface chrome should do the same rather than
duplicating brand values.

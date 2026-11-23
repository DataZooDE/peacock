# A Triton chat card shows none of peacock's render text

**Symptom.** #46 put a "Rows truncated: …" sentence in `render_report`'s
MCP text content, yet the Teams card for the same oversized report said
nothing about partial data.

**Why.** For an inline `report` component, Triton (`triton-chat-msteams`
`report_native_chart` / `reply_image_url`) dispatches `render_report` and
reads only `_meta.chart` (native `Chart.*`) and the PNG (fallback image, and
the only thing Google Chat shows). The card's text comes from the AGENT's
surface. Peacock's `content[].text` and `structuredContent` never reach it.

**Fix.** Put chat-visible facts where the card reads them: the capped view's
chart title gains "(Partial data: first N rows…)" in `compose`, so both the
painted SVG/PNG and `_meta.chart.title` carry it.

**Recognise it.** Anything peacock wants a chat user to see must be in the
chart (title / painted image), not in the tool result's text. A separate
TextBlock would need a Triton change (read e.g. a `_meta` note).

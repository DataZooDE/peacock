# anofox's SVG has no background — a dark PNG needs its card painted in

**Symptom.** The dark variant rendered correctly as SVG but its PNG showed light text and black
gridlines on white: anofox draws on a transparent canvas (the page or card behind it supplies the
colour), and image viewers / chat clients paint white behind a transparent PNG.

**Fix.** `chart::svg_to_png_on` fills the pixmap with the variant's card colour before
rasterizing (light: `--pk-surface`, dark: anofox's dark card `#141414`). The inline SVG in the
iframe stays transparent and sits on the runtime's own card.

**Related.** Lifting a brand palette to WCAG contrast on the dark card can collapse two dark
shades of one hue onto the same colour (company-a's `#6b3fa0` and `#4b2c70` both became
`#7a48b6`). `peacock-theme`'s palette lift keeps every series ≥ 24 RGB units from the earlier
ones, moving a clashing colour further in lightness.

**Recognise it.** A dark chart PNG looks washed out or unreadable in a chat client, but fine in
the browser.

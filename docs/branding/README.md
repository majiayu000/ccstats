# Branding Assets

This directory stores icon exploration and production-ready exports for `ccstats`.

## Concepts

- `icon-concept-a.svg`: dark analytics style with trend line.
- `icon-concept-b.svg`: selected concept (C-ring + bars).
- `icon-concept-c.svg`: token stack style.

## Final Assets

Final selected icon is based on concept B in `final/icon.svg`.

### README Card

- `readme-card.png`: README hero card.
- `readme-card.svg`: deterministic text and layout overlay used to regenerate the card.
- `readme-card-background.png`: imagegen-generated dashboard background for the card.

### Exported Files

- `final/icon.svg`: master vector source.
- `final/icon-1024.png`: app store / high-res marketing.
- `final/icon-512.png`: desktop app / release artwork.
- `final/icon-256.png`: medium app icon.
- `final/icon-180.png`: Apple touch icon.
- `final/icon-128.png`: launcher / docs previews.
- `final/icon-64.png`: small app icon.
- `final/icon-32.png`: web tab / file association.
- `final/icon-16.png`: tiny fallback icon.
- `final/favicon.ico`: multi-size favicon bundle.

## Regeneration

If you update `final/icon.svg`, regenerate PNG/ICO assets with:

```bash
BASE="docs/branding"
FINAL="$BASE/final"
mkdir -p "$FINAL"
for SIZE in 1024 512 256 180 128 64 32 16; do
  rsvg-convert -w "$SIZE" -h "$SIZE" "$FINAL/icon.svg" -o "$FINAL/icon-${SIZE}.png"
done
magick "$FINAL/icon-16.png" "$FINAL/icon-32.png" "$FINAL/icon-64.png" "$FINAL/favicon.ico"
```

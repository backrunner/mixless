---
name: mixless-brand
description: Apply the mixless brand across its native app, website, documentation, README, installers, and product imagery. Use for mixless branding, typography, marketing surfaces, and product screenshots.
---

# mixless brand

Use `mixless` in lowercase everywhere people see the product name, including sentence openings, menus, window titles, accessibility labels, metadata, social cards, documentation, and README. `AutoMix` is a feature name and retains its spelling.

## Wordmark and typography

- The standalone wordmark is `mixless.`: off-white letters, yellow `#f5d90a` final dot. In prose write `mixless`, without an added dot.
- Use Helvetica Neue with system sans-serif fallbacks; semibold (600). Website wordmark tracking is `-0.025em`, with normal kerning. Avoid stretched letters, spaces between letters, uppercase transforms, or excessively tight headline tracking inherited by the wordmark.
- Native GPUI uses its shaped Helvetica Neue text at natural spacing: 15 px in the toolbar, 24 px in About. Render through `apps/desktop/src/branding.rs` so the spelling and dot stay consistent.
- In About, keep the shared icon and 24 px wordmark beside the version and release channel. Place build diagnostics below a subtle divider; retain useful website, guide, source and license links, copyable version details, keyboard focus and close shortcuts.
- README renderers do not reliably support custom tracking. Use a normal lowercase heading; never approximate tracking with inserted spaces or Unicode characters.

## Visual language

- Use the existing rounded M icon from `assets/branding/app-icon-1024.png`. Keep its yellow-to-orange enamel treatment and proportions; do not invent another mark.
- Website favicons use a rounded rectangle with actual transparent corners and a narrow transparent margin. Generate 32 px and 64 px PNGs from the master with `apps/site/scripts/sync-assets.mjs`; CSS border radius does not apply to browser tab icons.
- README uses the generated `assets/branding/readme-icon.png` and `assets/screenshots/workspace-readme.webp`, both with transparent rounded corners. Link the screenshot to the unmodified native PNG. GitHub strips inline styling, so never rely on CSS to create these corners; the same asset script regenerates them.
- Palette: graphite surfaces (`#0c0c0e`, `#141416`), warm white (`#f0ede6`), amber (`#ffb224`), orange-red (`#ff5238`), with yellow reserved for the wordmark dot. Preserve distinct semantic waveform colors.
- Favor precise spacing, thin borders, clear hierarchy, and subtle depth that feels related to the native mixing workspace. Put real product views at the center of marketing pages.
- Motion should echo music or the mark: a brief entrance, a traced continuous line, or a transition playhead. Keep text readable and controls stable; respect reduced motion and provide a pause control for continuous decorative animation.
- Backgrounds can layer broad, low-contrast amber and copper gradients over graphite, with a faint muted violet countertone. Landing ambient light may breathe slowly (18–24 seconds per direction) through small opacity/transform changes; avoid animated blur or saturated moving blobs. Include it in the landing pause control, retain a static reduced-motion appearance, and keep reading pages still.

## Product images and downloads

- Capture the current built native app at Retina resolution after visible UI changes. Record capture date, source revision/build and dimensions in `assets/screenshots/capture.json`. Never repaint outdated screenshots or upscale them and call them new.
- Inspect screenshots for stale branding, private paths, errors, open dialogs, clipped controls, and misleading states. Preserve original PNGs; generate responsive WebP derivatives with `apps/site/scripts/sync-assets.mjs`.
- Download CTAs must resolve the latest published stable macOS installer, falling back to the latest beta only when no stable release exists. Identify beta clearly and offer other releases, checksums, and source. Do not hardcode a release version into the primary CTA or call an API failure “no stable release.”

Case-sensitive asset filenames, application support paths, bundle identifiers, release keys and existing release URLs are compatibility contracts, not prose. Inspect their consumers before renaming them; display names can change independently. Historical generated-asset records remain historical evidence.

For changes, inspect the native toolbar/About, both language homepages, a docs page, mobile navigation, and social card. Run the appropriate build/content checks, verify links, and visually inspect at desktop/mobile widths with reduced motion.

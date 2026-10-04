# mixless website

The official website and user guide, built with **svedocs / svedocs-cli 0.2.2** and SvelteKit. Pages are prerendered with a custom dark theme, product landing, local search, and English/Chinese content. A small Cloudflare Worker resolves current GitHub releases for direct downloads; it needs no credentials.

## Develop and validate

Requires Node.js 22.12+ and the pnpm version pinned in `package.json`.

```sh
pnpm install --frozen-lockfile
pnpm dev
pnpm check
pnpm check:content
pnpm test:releases
pnpm build
node scripts/serve-build.mjs 4174
pnpm exec playwright install chromium
pnpm test:e2e
```

Run these commands in `apps/site`, or use `pnpm -C apps/site` from the repository root. `check:content` validates internal links, assets, and complete translation coverage. CI runs the same checks, build, and desktop/mobile browser smoke tests.

## Content and design

- `content/pages/index.md` owns home metadata and the Markdown representation for agents. Other standalone pages are privacy and support.
- `content/docs` contains the user guide. Chinese mirrors the same paths under `zh` in both content roots. English routes are `/` and `/docs`; Chinese routes are `/zh` and `/docs/zh`.
- `src/lib/landing` owns the landing, responsive screenshots, and interactive transition illustration. The illustration is schematic and does not play audio. Continuous motion can be paused and honors reduced motion.
- The [brand skill](../../.agents/skills/mixless-brand/SKILL.md) defines lowercase spelling, wordmark tracking, palette, and image capture rules. `Wordmark.svelte` is shared by the navbar, footer, and download panel.
- `src/lib/theme` replaces navigation and footer through the public svedocs component contract.
- `src/lib/styles/theme.css` restyles reading surfaces and tools. The framework CSS retains accessible dialog and content behavior; the default home, pixel decoration, header and footer are not used.
- Reading pages use the framework's `plain` mode and open article columns. The hero divider is the single heading separator; prose headings have no decorative border or extra top padding. Keep table, code, navigation and focus boundaries intact. `tests/reading.spec.ts` audits every English/Chinese reading route at desktop/mobile widths under both OS color preferences and saves screenshots plus computed style evidence.
- `src/lib/messages.ts` contains translated interface and landing copy. Use `resolveLocalizedHref` for internal component links.

The root and catch-all routes share `SitePage.svelte`, preserving svedocs page loading, search, metadata, alternate languages and compiled Markdown. Keep the generated Markdown-twin and `llms.txt` routes connected.

## Product assets

```sh
pnpm assets
```

This regenerates the committed site icon, transparent rounded favicons, rounded README icon and screenshot, responsive WebP screenshots, intrinsic-size metadata, and social card from the repository's `assets/branding` and `assets/screenshots`. Native capture provenance is recorded in `assets/screenshots/capture.json`; the workspace source must be at least 2560 px wide. Keep source screenshots real; inspect the generated files after an asset change. README and favicon corners are baked into their alpha channels because those surfaces do not support CSS rounding. Fonts use the system stack and do not contact a font CDN.

## Hosting

`pnpm build` writes the static site to `build/`. Wrangler publishes those files using Cloudflare Workers Static Assets. `wrangler.jsonc` owns the `mixless-site` Worker and its custom domain, `mixless.alkinum.com`, in the Alkinum account. `worker.mjs` handles `/download` and `/api/releases`; other requests use the `ASSETS` binding. No secrets are needed.

After the validation commands above pass, deploy from `apps/site` using an authenticated Wrangler session:

```sh
pnpm exec wrangler whoami
pnpm run deploy
```

The local static host (`node scripts/serve-build.mjs 4174`) uses the same release resolver. Plain static hosting or `svedocs preview` alone does not serve the download endpoints. Use `pnpm exec wrangler dev` after building to validate the full Worker locally.

For a deployment preview without publishing, run `pnpm build && pnpm exec wrangler deploy --dry-run --no-autoconfig`. Credentials stay in Wrangler's local login or the deployment environment; never put them in the repository. Deployment is manual; the website CI only validates and uploads a build artifact.

The default canonical origin is `https://mixless.alkinum.com`. To target another domain, set `SITE_URL` at build time:

```sh
SITE_URL=https://example.com pnpm build
```

Wrangler serves unknown paths with `404.html` **and HTTP 404** and redirects HTML paths to extensionless URLs with a trailing slash, matching svedocs static canonicals and sitemap URLs. The server hook writes the page language into prerendered HTML before JavaScript runs. The output includes `sitemap.xml`, `robots.txt`, per-page Markdown, `llms.txt`, and `llms-full.txt`. No SPA fallback or server-side AI/search endpoint is needed.

Building alone does not publish the site. `pnpm run deploy` invokes this project's deployment script and attaches the configured custom domain; plain `pnpm deploy` is pnpm's separate workspace deployment command. After deploying, verify the English and Chinese homepages, a docs page and local search, missing-route status, and sitemap on the production domain. Changing `SITE_URL` also requires matching the custom domain in `wrangler.jsonc`.

Run the SEO checks against the actual Cloudflare asset router after building:

```sh
pnpm exec wrangler dev --ip 127.0.0.1 --port 4175
# In another terminal:
SITE_TEST_URL=http://127.0.0.1:4175 pnpm test:seo
# After publishing:
SITE_TEST_URL=https://mixless.alkinum.com pnpm test:seo
```

The checks cover all 24 public routes with JavaScript disabled and a Googlebot user agent, title uniqueness, descriptions, canonical URLs without redirects, reciprocal hreflang, structured data, social assets, hydrated titles, language switching, and HTTP 404. A Googlebot user agent is a response-parity check; actual Google crawl and indexing state must be checked in Search Console. Each run attaches a per-page `seo-audit.json` report to the Playwright results.

## Release downloads

`/download` redirects directly to the latest published stable macOS DMG. If GitHub has no stable release (404 from its `/latest` endpoint), it selects the latest published beta with a complete installer. Drafts and alpha releases are excluded. `/download?channel=beta` explicitly selects beta; `/download?asset=checksum` follows the recommended release's SHA256SUMS. Existing release asset filenames remain case sensitive.

The GitHub API is cached at the edge for five minutes; downloads use non-cacheable redirects so release updates do not require a site rebuild. `/api/releases` supplies the displayed version, channel, size, notes and checksum availability. During an API failure, the UI explains that details are unavailable and direct downloads fall back to GitHub Releases. An incomplete stable release is an error, not a reason to silently substitute beta.

`pnpm test:releases` verifies selection and redirects with synthetic feeds. Browser tests mock release metadata for repeatability; separately probe `/api/releases` and `/download` against GitHub and confirm the selected real DMG URL after deploying.

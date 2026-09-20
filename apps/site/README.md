# Mixless website

The official website and user guide, built with **svedocs / svedocs-cli 0.2.1** and SvelteKit. The site is fully static, with a custom dark theme, product landing, local search, and English/Chinese content. No runtime services or credentials are required.

## Develop and validate

Requires Node.js 22.12+ and the pnpm version pinned in `package.json`.

```sh
pnpm install --frozen-lockfile
pnpm dev
pnpm check
pnpm check:content
pnpm build
pnpm preview
pnpm exec playwright install chromium
pnpm test:e2e
```

Run these commands in `apps/site`, or use `pnpm -C apps/site` from the repository root. `check:content` validates internal links, assets, and complete translation coverage. CI runs the same checks, build, and desktop/mobile browser smoke tests.

## Content and design

- `content/pages/index.md` owns home metadata and the Markdown representation for agents. Other standalone pages are privacy and support.
- `content/docs` contains the user guide. Chinese mirrors the same paths under `zh` in both content roots. English routes are `/` and `/docs`; Chinese routes are `/zh` and `/docs/zh`.
- `src/lib/landing` owns the landing and the interactive transition illustration. The illustration is schematic and does not play audio.
- `src/lib/theme` replaces navigation and footer through the public svedocs component contract.
- `src/lib/styles/theme.css` restyles reading surfaces and tools. The framework CSS retains accessible dialog and content behavior; the default home, pixel decoration, header and footer are not used.
- `src/lib/messages.ts` contains translated interface and landing copy. Use `resolveLocalizedHref` for internal component links.

The root and catch-all routes share `SitePage.svelte`, preserving svedocs page loading, search, metadata, alternate languages and compiled Markdown. Keep the generated Markdown-twin and `llms.txt` routes connected.

## Product assets

```sh
pnpm assets
```

This regenerates the committed site icon, WebP screenshots and social card from the repository's `assets/branding` and `assets/screenshots`. Keep source screenshots real; inspect the generated files after an asset change. Fonts use the system stack and do not contact a font CDN.

## Hosting

`pnpm build` writes the static site to `build/`. Wrangler publishes those files using Cloudflare Workers Static Assets. `wrangler.jsonc` owns the `mixless-site` Worker and its custom domain, `mixless.alkinum.com`, in the Alkinum account. The site does not need a Worker script, runtime bindings, or secrets.

After the validation commands above pass, deploy from `apps/site` using an authenticated Wrangler session:

```sh
pnpm exec wrangler whoami
pnpm deploy
```

For a deployment preview without publishing, run `pnpm build && pnpm exec wrangler deploy --dry-run --no-autoconfig`. Credentials stay in Wrangler's local login or the deployment environment; never put them in the repository. Deployment is manual; the website CI only validates and uploads a build artifact.

The default canonical origin is `https://mixless.alkinum.com`. To target another domain, set `SITE_URL` at build time:

```sh
SITE_URL=https://example.com pnpm build
```

Wrangler serves unknown paths with `404.html` **and HTTP 404** and redirects HTML paths to extensionless URLs without a trailing slash. The output includes `sitemap.xml`, `robots.txt`, per-page Markdown, `llms.txt`, and `llms-full.txt`. No SPA fallback or server-side AI/search endpoint is needed.

Building alone does not publish the site. `pnpm deploy` publishes the build and attaches the configured custom domain. After deploying, verify the English and Chinese homepages, a docs page and local search, missing-route status, and sitemap on the production domain. Changing `SITE_URL` also requires matching the custom domain in `wrangler.jsonc`.

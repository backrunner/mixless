import { test, expect } from '@playwright/test';
import { mkdirSync, writeFileSync } from 'node:fs';

const slugs = ['', 'audio', 'automix', 'decks', 'development', 'library', 'midi', 'shortcuts', 'storage'];
const routes = ['/', '/zh/', '/privacy/', '/support/', '/zh/privacy/', '/zh/support/',
  ...['/docs', '/docs/zh'].flatMap(prefix => slugs.map(slug => `${prefix}/${slug ? `${slug}/` : ''}`))];

test('all sitemap pages expose consistent SEO without JavaScript, including Googlebot', async ({ browser, baseURL }, info) => {
  test.setTimeout(120_000);
  const context = await browser.newContext({ baseURL, javaScriptEnabled: false, userAgent: 'Googlebot' });
  const page = await context.newPage();
  const failures: string[] = [];
  const report: object[] = [];
  const titles = new Set<string>();
  const descriptions = new Set<string>();
  try {
    const sitemap = await context.request.get('/sitemap.xml');
    expect(sitemap.status()).toBe(200);
    expect(sitemap.headers()['content-type']).toContain('xml');
    await page.goto('/sitemap.xml');
    const entries = await page.evaluate(() => Array.from(document.getElementsByTagName('url')).map(node => ({
      loc: node.getElementsByTagName('loc')[0].textContent!,
      alternates: Array.from(node.getElementsByTagNameNS('http://www.w3.org/1999/xhtml', 'link'))
        .map(link => ({ lang: link.getAttribute('hreflang'), href: link.getAttribute('href')! }))
    })));
    expect(entries.map(entry => new URL(entry.loc).pathname).sort()).toEqual([...routes].sort());
    const origin = new URL(entries[0].loc).origin;
    const robots = await context.request.get('/robots.txt');
    expect(robots.status()).toBe(200);
    expect(await robots.text()).toContain(`Sitemap: ${origin}/sitemap.xml`);
    expect(await robots.text()).not.toMatch(/Disallow:\s*\//);
    for (const entry of entries) {
      const path = new URL(entry.loc).pathname;
      const language = path === '/zh/' || path.startsWith('/zh/') || path.startsWith('/docs/zh/') ? 'zh-CN' : 'en';
      const response = await page.goto(path);
      expect(response?.status(), path).toBe(200);
      const metadata = await page.evaluate(() => ({
        title: document.title,
        titles: document.head.querySelectorAll('title').length,
        description: document.head.querySelector('meta[name="description"]')?.getAttribute('content'),
        descriptions: document.head.querySelectorAll('meta[name="description"]').length,
        lang: document.documentElement.lang,
        canonical: Array.from(document.head.querySelectorAll('link[rel="canonical"]')).map(el => el.getAttribute('href')),
        alternates: Array.from(document.head.querySelectorAll('link[hreflang]')).map(el => ({ lang: el.getAttribute('hreflang'), href: el.getAttribute('href') })),
        ogTitle: document.head.querySelector('meta[property="og:title"]')?.getAttribute('content'),
        twitterTitle: document.head.querySelector('meta[name="twitter:title"]')?.getAttribute('content'),
        ogUrl: document.head.querySelector('meta[property="og:url"]')?.getAttribute('content'),
        image: document.head.querySelector('meta[property="og:image"]')?.getAttribute('content'),
        robots: document.head.querySelector('meta[name="robots"]')?.getAttribute('content') ?? '',
        h1: document.querySelectorAll('main h1').length,
        jsonLd: Array.from(document.head.querySelectorAll('script[type="application/ld+json"]')).map(el => JSON.parse(el.textContent!))
      }));
      report.push({ path, status: response?.status(), finalUrl: page.url(), ...metadata });
      // Collect every page's problems so a live audit does not stop at the first failure.
      const check = (ok: boolean, message: string) => { if (!ok) failures.push(`${path}: ${message}`); };
      check(response?.request().redirectedFrom() === null, 'sitemap URL redirects');
      check(new URL(page.url()).pathname === path, 'final path differs from sitemap');
      check(metadata.lang === language, `HTML language ${metadata.lang}, expected ${language}`);
      check(metadata.titles === 1 && metadata.title.includes('mixless'), 'missing, duplicate or unbranded title');
      check(!titles.has(metadata.title), 'duplicate page title');
      titles.add(metadata.title);
      check(metadata.descriptions === 1 && Boolean(metadata.description), 'missing or duplicate description');
      check(!descriptions.has(metadata.description!), 'duplicate page description');
      descriptions.add(metadata.description!);
      check(metadata.canonical.length === 1 && metadata.canonical[0] === entry.loc, 'canonical differs from sitemap');
      check(metadata.ogUrl === entry.loc, 'og:url differs from canonical');
      check(metadata.ogTitle === metadata.title && metadata.twitterTitle === metadata.title, 'social titles differ from title');
      check(!/noindex|none/i.test(metadata.robots), 'public page is noindex');
      check(metadata.h1 === 1, 'page needs one main H1');
      check(JSON.stringify(metadata.alternates) === JSON.stringify(entry.alternates), 'HTML/sitemap hreflang differs');
      check(metadata.alternates.every(link => entries.some(candidate => candidate.loc === link.href)), 'hreflang points outside sitemap');
      const website: Record<string, unknown>[] = [];
      const collectWebsites = (value: unknown) => {
        if (Array.isArray(value)) value.forEach(collectWebsites);
        else if (value && typeof value === 'object') {
          const node = value as Record<string, unknown>;
          if (node['@type'] === 'WebSite') website.push(node);
          Object.values(node).forEach(collectWebsites);
        }
      };
      collectWebsites(metadata.jsonLd);
      check(website.length === 1 && website[0].name === 'mixless' &&
        typeof website[0].url === 'string' && new URL(website[0].url).href === `${origin}/`,
      'missing or duplicated site-name structured data');
      const article = metadata.jsonLd.find(data => ['WebPage', 'TechArticle'].includes(data['@type']));
      check(article?.headline === metadata.title && article?.url === entry.loc && article?.inLanguage === language, 'page structured data differs from HTML');
      check(article?.author?.['@type'] === 'Organization', 'Alkinum must be an Organization');
      if (metadata.image) {
        const image = await context.request.get(new URL(metadata.image).pathname);
        check(image.status() === 200 && image.headers()['content-type'].startsWith('image/'), 'social image unavailable');
      }
      check(!/noindex|none/i.test(response?.headers()['x-robots-tag'] ?? ''), 'response is noindex');
    }
    mkdirSync(info.outputDir, { recursive: true });
    const reportPath = info.outputPath('seo-audit.json');
    writeFileSync(reportPath, JSON.stringify({ baseURL, pages: report, failures }, null, 2));
    await info.attach('seo-audit.json', { path: reportPath, contentType: 'application/json' });
    expect(failures).toEqual([]);
  } finally {
    await context.close();
  }
});

test('ordinary browser hydration and language navigation preserve server titles', async ({ page, request }) => {
  for (const path of ['/', '/zh/', '/docs/', '/docs/zh/automix/']) {
    const response = await request.get(path);
    const raw = await response.text();
    await page.goto(path);
    await page.waitForFunction(() => document.documentElement.hasAttribute('data-svedocs-route'));
    const title = await page.title();
    expect(raw).toContain(`<title>${title}</title>`);
    await expect(page.locator('head title')).toHaveCount(1);
    await expect(page.locator('meta[property="og:title"]')).toHaveAttribute('content', title);
    if (path === '/' || path === '/zh/') {
      expect(title).toBe(path === '/' ? 'mixless — Free DJ Software for macOS' : 'mixless — 免费 macOS DJ 混音软件');
    }
    await page.locator('.mx-language').click();
    await expect(page.locator('html')).toHaveAttribute('lang', path.includes('zh') ? 'en' : 'zh-CN');
    await expect(page.locator('head title')).toHaveCount(1);
  }
  const missing = await request.get('/seo-audit-missing-page/');
  expect(missing.status()).toBe(404);
  await page.goto('/seo-audit-missing-page/');
  await expect(page.locator('meta[name="robots"]')).toHaveAttribute('content', 'noindex');
  for (const path of ['/brand/favicon-32.png', '/brand/favicon-64.png', '/images/social.png']) {
    const asset = await request.get(path);
    expect(asset.status()).toBe(200);
    expect(asset.headers()['content-type']).toContain('image/png');
  }
});

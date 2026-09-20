import { test, expect, type Page } from '@playwright/test';

async function hydrated(page: Page) {
  await page.waitForFunction(() => document.documentElement.hasAttribute('data-svedocs-route'));
}

async function noOverflow(page: Page) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
}

for (const locale of ['en', 'zh'] as const) {
  const home = locale === 'zh' ? '/zh/' : '/';
  const docs = locale === 'zh' ? '/docs/zh' : '/docs';

  test(`${locale}: home, transitions, language and metadata`, async ({ page }, info) => {
    const errors: string[] = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.goto(home);
    await hydrated(page);
    await expect(page.locator('main')).toHaveCount(1);
    await expect(page.locator('h1')).toContainText(locale === 'zh' ? '自然接着流动' : 'One continuous flow');
    await expect(page.locator('html')).toHaveAttribute('lang', locale === 'zh' ? 'zh-CN' : 'en');
    await expect(page.locator('link[rel="canonical"]')).toHaveAttribute('href', new RegExp(`https://mixless.alkinum.com${locale === 'zh' ? '/zh' : '/'}/*$`));
    await expect(page.locator('link[hreflang="zh-CN"]')).toHaveCount(1);
    await expect(page.locator('meta[name="twitter:card"]')).toHaveAttribute('content', 'summary_large_image');
    await expect(page.locator('.mx-product img')).toBeVisible();
    expect(await page.locator('.mx-product img').evaluate((img: HTMLImageElement) => img.naturalWidth)).toBe(1680);
    await noOverflow(page);
    await page.screenshot({ path: info.outputPath(`${locale}-home.png`), fullPage: true });

    const cut = page.getByRole('button', { name: locale === 'zh' ? 'Drop 切换' : 'Drop cut', exact: true });
    await cut.click();
    await expect(cut).toHaveAttribute('aria-pressed', 'true');
    await expect(page.locator('.mx-transition-description')).toContainText('build-up');
    await expect(page.locator('.mx-transition')).toHaveAttribute('data-mode', 'cut');

    await page.locator('.mx-language').click();
    await expect(page).toHaveURL(locale === 'zh' ? /\/$/ : /\/zh\/?$/);
    await expect(page.locator('h1')).toContainText(locale === 'zh' ? 'Your tracks.' : '让你的音乐');
    expect(errors).toEqual([]);
  });

  test(`${locale}: guide, local search, mobile navigation and anchors`, async ({ page }, info) => {
    await page.goto(`${docs}/`);
    await hydrated(page);
    await expect(page.locator('h1')).toHaveText(locale === 'zh' ? '第一次混音' : 'Your first mix');
    await page.locator('.sd-search-trigger').click();
    await expect(page.getByRole('dialog')).toBeVisible();
    await page.getByRole('combobox').fill('MIDI');
    const result = page.locator(`.sd-search-results a[href^="${docs}/midi"]`).first();
    await expect(result).toBeVisible();
    const results = await page.locator('.sd-search-results a').evaluateAll(links => links.map(a => a.getAttribute('href')!));
    expect(results.length).toBeGreaterThan(0);
    expect(results.every(href => locale === 'zh' ? href.includes('/zh/') : !href.includes('/zh/'))).toBe(true);
    await result.click();
    await expect(page).toHaveURL(new RegExp(`${docs}/midi`));
    await expect(page.getByRole('dialog')).toHaveCount(0);
    await noOverflow(page);
    await page.screenshot({ path: info.outputPath(`${locale}-guide.png`), fullPage: true });

    // Language switching keeps the current article rather than returning home.
    await page.locator('.mx-language').click();
    await expect(page).toHaveURL(new RegExp(locale === 'zh' ? '/docs/midi/?$' : '/docs/zh/midi/?$'));
    await page.goto(`${docs}/`);
    await hydrated(page);
    if (info.project.name === 'mobile') {
      await page.locator('.mx-menu-button').click();
      await expect(page.locator('.mx-menu-button')).toHaveAttribute('aria-expanded', 'true');
      await page.locator(`.mx-mobile-menu .sd-mobile-docnav a[href="${docs}/automix"]`).click();
      await expect(page).toHaveURL(new RegExp(`${docs}/automix`));
      await expect(page.locator('.mx-menu-button')).toHaveAttribute('aria-expanded', 'false');
    }
    const heading = page.locator('.sd-prose h2').first();
    const id = await heading.getAttribute('id');
    await page.goto(`${page.url().split('#')[0]}#${id}`);
    await expect(heading).toBeInViewport();
    expect(await heading.evaluate(el => el.getBoundingClientRect().top)).toBeGreaterThanOrEqual(65);
    await page.keyboard.press('Control+k');
    await expect(page.getByRole('dialog')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('dialog')).toHaveCount(0);
  });
}

test('all public routes, assets, agent endpoints, missing routes and reduced motion', async ({ page, request }, info) => {
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  for (const prefix of ['', '/zh']) {
    for (const slug of ['/', '/library/', '/automix/', '/decks/', '/audio/', '/midi/', '/shortcuts/', '/storage/', '/development/']) {
      const response = await page.goto(`/docs${prefix}${slug}`);
      expect(response?.status()).toBe(200);
      await expect(page.locator('main h1')).toBeVisible();
      await noOverflow(page);
    }
    for (const slug of ['privacy', 'support']) {
      const response = await page.goto(`${prefix}/${slug}/`);
      expect(response?.status()).toBe(200);
      await expect(page.locator('main h1')).toBeVisible();
      await noOverflow(page);
    }
  }
  for (const [path, content] of [
    ['/sitemap.xml', '<urlset'], ['/robots.txt', 'Sitemap:'],
    ['/llms.txt', 'Mixless'], ['/llms-full.txt', 'Mixless'],
    ['/docs/zh/midi/index.md', '# MIDI 控制器']
  ]) {
    const response = await request.get(path);
    expect(response.status()).toBe(200);
    expect(await response.text()).toContain(content);
  }
  const missing = await page.goto('/no-such-page/');
  expect(missing?.status()).toBe(404);
  await expect(page.getByRole('heading', { name: 'Page not found' })).toBeVisible();
  await noOverflow(page);
  await page.emulateMedia({ reducedMotion: 'reduce', colorScheme: 'light' });
  await page.goto('/');
  await hydrated(page);
  expect(await page.locator('.sd-root').evaluate(el => getComputedStyle(el).backgroundColor)).toBe('rgb(12, 12, 14)');
  expect(await page.locator('.mx-track i').first().evaluate(el => getComputedStyle(el).transitionDuration)).toBe('0s');
  await page.setViewportSize({ width: 360, height: 780 });
  await noOverflow(page);
  await page.screenshot({ path: info.outputPath('narrow-home.png'), fullPage: true });
  expect(errors).toEqual([]);
});

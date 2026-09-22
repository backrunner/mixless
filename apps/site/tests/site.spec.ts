import { test, expect, type Page } from '@playwright/test';

const beta = { version: 'v0.1.0-beta.2', channel: 'beta', size: 33843871, bundled: { url: 'https://github.com/backrunner/mixless/releases/download/v0.1.0-beta.2/Mixless-macOS-0.1.0-beta.2-with-models.dmg', size: 199456507 }, notes: 'https://github.com/backrunner/mixless/releases/tag/v0.1.0-beta.2', checksum: 'https://github.com/backrunner/mixless/releases/download/v0.1.0-beta.2/SHA256SUMS.txt' };
test.beforeEach(async ({ page }) => {
  // Deterministic browser coverage; the release resolver is also tested against live GitHub separately.
  await page.route('**/api/releases', route => route.fulfill({ json: { stable: null, beta, recommended: beta } }));
});

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
    // With width-descriptor srcset, naturalWidth is density-corrected CSS pixels.
    const sourceWidth = await page.locator('.mx-product img').evaluate(async (img: HTMLImageElement) => {
      const bitmap = await createImageBitmap(await (await fetch(img.currentSrc)).blob());
      const width = bitmap.width;
      bitmap.close();
      return width;
    });
    expect(sourceWidth).toBeGreaterThanOrEqual(840);
    await noOverflow(page);
    await expect(page).toHaveTitle(/mixless/);
    await expect(page.locator('.mx-release-badge')).toHaveText(locale === 'zh' ? 'Beta 测试版' : 'Beta');
    await page.screenshot({ path: info.outputPath(`${locale}-home.png`), fullPage: true, animations: 'disabled' });

    const cut = page.getByRole('button', { name: locale === 'zh' ? 'Drop 切换' : 'Drop cut', exact: true });
    await cut.click();
    await expect(cut).toHaveAttribute('aria-pressed', 'true');
    await expect(page.locator('.mx-transition-description')).toContainText('build-up');
    await expect(page.locator('.mx-transition')).toHaveAttribute('data-mode', 'cut');
    await page.locator('.mx-motion-toggle').click();
    await expect(page.locator('.mx-landing')).toHaveAttribute('data-motion', 'paused');
    expect(await page.locator('.mx-playhead').evaluate(el => getComputedStyle(el).animationPlayState)).toBe('paused');
    for (const layer of ['::before', '::after']) {
      const ambient = await page.locator('.mx-hero').evaluate((el, pseudo) => {
        const style = getComputedStyle(el, pseudo);
        return { state: style.animationPlayState, duration: parseFloat(style.animationDuration) };
      }, layer);
      expect(ambient.state).toBe('paused');
      expect(ambient.duration).toBeGreaterThanOrEqual(18);
    }
    await expect(page.locator('.mx-actions a').first()).toHaveAttribute('href', '/download');
    await page.locator('.mx-download-caption a').click();
    await expect(page.locator('#download-title')).toBeInViewport();
    expect(await page.locator('#downloads').evaluate(el => getComputedStyle(el).opacity)).toBe('1');
    await expect(page.locator('.mx-download-options a[href="/download?channel=beta"]')).toBeVisible();
    await expect(page.locator('.mx-download-options a[href="/download?variant=bundled"]')).toContainText(locale === 'zh' ? '离线分轨' : 'Offline stem analysis');
    await page.locator('#downloads').screenshot({ path: info.outputPath(`${locale}-downloads.png`), animations: 'disabled' });
    await expect(page.locator('.mx-download-options a[href="/download?asset=checksum"]')).toBeVisible();

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
    await page.screenshot({ path: info.outputPath(`${locale}-guide.png`), fullPage: true, animations: 'disabled' });

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
    ['/llms.txt', 'mixless'], ['/llms-full.txt', 'mixless'],
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
  for (const size of [32, 64]) {
    const favicon = page.locator(`link[rel="icon"][sizes="${size}x${size}"]`);
    await expect(favicon).toHaveAttribute('href', new RegExp(`/brand/favicon-${size}\\.png$`));
    const response = await request.get((await favicon.getAttribute('href'))!);
    expect(response.status()).toBe(200);
    expect(response.headers()['content-type']).toContain('image/png');
  }
  expect(await page.locator('.sd-root').evaluate(el => getComputedStyle(el).backgroundColor)).toBe('rgb(12, 12, 14)');
  expect(await page.locator('.mx-track i').first().evaluate(el => getComputedStyle(el).transitionDuration)).toBe('0s');
  expect(await page.locator('.mx-playhead').evaluate(el => getComputedStyle(el).animationName)).toBe('none');
  expect(await page.locator('.mx-flow-line').evaluate(el => getComputedStyle(el).animationName)).toBe('none');
  for (const layer of ['::before', '::after']) {
    expect(await page.locator('.mx-hero').evaluate((el, pseudo) => getComputedStyle(el, pseudo).animationName, layer)).toBe('none');
  }
  await page.setViewportSize({ width: 360, height: 780 });
  await noOverflow(page);
  await page.screenshot({ path: info.outputPath('narrow-home.png'), fullPage: true, animations: 'disabled' });
  expect(errors).toEqual([]);
});

test('stable release presentation and service failure remain usable', async ({ page }) => {
  const stable = { ...beta, version: 'v1.0.0', channel: 'stable' };
  await page.route('**/api/releases', route => route.fulfill({ json: { stable, beta, recommended: stable } }));
  await page.goto('/');
  await expect(page.locator('.mx-release-badge')).toHaveText('Stable');
  await expect(page.locator('.mx-release-status')).toContainText('v1.0.0');
  await expect(page.locator('.mx-download-options a[href="/download?channel=beta"]')).toHaveCount(1);
  await page.route('**/api/releases', route => route.fulfill({ status: 503, json: { error: 'unavailable' } }));
  await page.reload();
  await expect(page.locator('.mx-release-status')).toContainText('Release details unavailable');
  await expect(page.locator('.mx-download-main a')).toHaveAttribute('href', '/download');
  await expect(page.locator('.mx-download-options a', { hasText: 'All versions' })).toHaveAttribute('href', 'https://github.com/backrunner/mixless/releases');
});


test('older releases do not advertise an unavailable model installer', async ({ page }) => {
  const standardOnly = { ...beta, bundled: null };
  await page.route('**/api/releases', route => route.fulfill({ json: { stable: null, beta: standardOnly, recommended: standardOnly } }));
  await page.goto('/');
  await expect(page.locator('.mx-release-status')).toContainText(beta.version);
  await expect(page.locator('.mx-download-options a[href*="variant=bundled"]')).toHaveCount(0);
  await expect(page.locator('.mx-download-main a')).toHaveAttribute('href', '/download');
});

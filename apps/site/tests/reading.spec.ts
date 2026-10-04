import { test, expect } from '@playwright/test';
import { mkdirSync, writeFileSync } from 'node:fs';

const slugs = ['', 'audio', 'automix', 'decks', 'development', 'library', 'midi', 'shortcuts', 'storage'];
const routes = ['/support/', '/zh/support/', '/privacy/', '/zh/privacy/',
  ...['/docs', '/docs/zh'].flatMap(prefix => slugs.map(slug => `${prefix}/${slug ? `${slug}/` : ''}`))];

for (const colorScheme of ['dark', 'light'] as const) {
  test(`all reading pages have open surfaces and no duplicate heading dividers (${colorScheme} OS)`, async ({ page }, info) => {
    test.setTimeout(120_000);
    await page.emulateMedia({ colorScheme, reducedMotion: 'reduce' });
    const report: object[] = [];
    const failures: string[] = [];
    for (const route of routes) {
      const response = await page.goto(route);
      expect(response?.status(), route).toBe(200);
      await page.waitForFunction(() => document.documentElement.hasAttribute('data-svedocs-route'));
      const styles = await page.evaluate(() => {
        const borders = (el: Element) => {
          const s = getComputedStyle(el);
          return [s.borderTopWidth, s.borderRightWidth, s.borderBottomWidth, s.borderLeftWidth];
        };
        const hero = document.querySelector('.sd-doc-header, .sd-page-hero')!;
        const first = document.querySelector('.sd-prose')?.firstElementChild;
        return {
          readingStyle: document.querySelector('.sd-root')?.getAttribute('data-reading-style'),
          shells: Array.from(document.querySelectorAll('.sd-doc, .sd-page')).map(el => ({
            borders: borders(el), shadow: getComputedStyle(el).boxShadow
          })),
          headings: Array.from(document.querySelectorAll('.sd-prose h2')).map(el => ({
            text: el.textContent, borders: borders(el), padding: getComputedStyle(el).paddingTop
          })),
          heroBorder: borders(hero),
          firstHeadingGap: first?.tagName === 'H2' ? first.getBoundingClientRect().top - hero.getBoundingClientRect().bottom : null,
          prosePadding: getComputedStyle(document.querySelector('.sd-prose')!).paddingTop,
          overflow: document.documentElement.scrollWidth > innerWidth,
          cornersVisible: Array.from(document.querySelectorAll('.sd-doc-corner')).some(el => getComputedStyle(el).display !== 'none'),
          gridVisible: getComputedStyle(document.querySelector('.sd-root')!, '::after').display !== 'none',
          kickerTransform: getComputedStyle(document.querySelector('.sd-page-hero .sd-kicker') ?? hero).textTransform
        };
      });
      const check = (ok: boolean, message: string) => { if (!ok) failures.push(`${route}: ${message}`); };
      check(styles.readingStyle === 'plain', 'reading surface is decorated');
      check(styles.shells.length === 1 && styles.shells.every(shell => shell.borders.every(border => border === '0px') && shell.shadow === 'none'), 'article has an outer frame');
      check(styles.headings.every(heading => heading.borders.every(border => border === '0px') && heading.padding === '0px'), 'prose heading has a decorative divider');
      check(JSON.stringify(styles.heroBorder) === JSON.stringify(['0px', '0px', '1px', '0px']), 'hero needs one bottom divider');
      check(styles.prosePadding === '0px' && (styles.firstHeadingGap === null || styles.firstHeadingGap <= 32), 'duplicate section spacing below hero');
      check(!styles.overflow && !styles.cornersVisible && !styles.gridVisible, 'overflow or pixel decorations');
      if (route.includes('support') || route.includes('privacy')) check(styles.kickerTransform === 'none', 'brand name is uppercase');
      report.push({ route, status: response?.status(), ...styles });
      await page.screenshot({ path: info.outputPath(`${route.slice(1).replaceAll('/', '-')}.png`), fullPage: true, animations: 'disabled' });
    }
    mkdirSync(info.outputDir, { recursive: true });
    const reportPath = info.outputPath('reading-audit.json');
    writeFileSync(reportPath, JSON.stringify({ colorScheme, viewport: page.viewportSize(), pages: report, failures }, null, 2));
    await info.attach('reading-audit.json', { path: reportPath, contentType: 'application/json' });
    expect(failures).toEqual([]);
  });
}

test('reading cleanup preserves keyboard focus and code/table boundaries', async ({ page }) => {
  await page.goto('/docs/development/');
  await page.waitForFunction(() => document.documentElement.hasAttribute('data-svedocs-route'));
  await page.keyboard.press('Tab');
  await expect(page.locator('.sd-skip')).toBeFocused();
  expect(await page.locator('.sd-skip').evaluate(el => {
    const s = getComputedStyle(el);
    return s.outlineStyle !== 'none' && parseFloat(s.outlineWidth) >= 2;
  })).toBe(true);
  expect(await page.locator('.sd-code').first().evaluate(el => parseFloat(getComputedStyle(el).borderTopWidth))).toBeGreaterThan(0);
  expect(await page.locator('.sd-prose th').first().evaluate(el => parseFloat(getComputedStyle(el).borderBottomWidth))).toBeGreaterThan(0);
  const copy = page.locator('.sd-code-copy').first();
  await expect(copy).toBeVisible();
});

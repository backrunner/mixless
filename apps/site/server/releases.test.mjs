import { test } from 'node:test';
import assert from 'node:assert/strict';
import { installer, loadReleases, handleReleaseRequest, RELEASES } from './releases.mjs';

function release(version, prerelease = false, date = '2026-09-20T12:00:00Z') {
  const tag = `v${version}`;
  return { tag_name: tag, prerelease, draft: false, published_at: date, html_url: `${RELEASES}/tag/${tag}`, assets: [
    { name: `Mixless-macOS-${version}.dmg`, size: 123456, browser_download_url: `${RELEASES}/download/${tag}/Mixless-macOS-${version}.dmg` },
    { name: 'SHA256SUMS.txt', size: 97, browser_download_url: `${RELEASES}/download/${tag}/SHA256SUMS.txt` }
  ] };
}
const stable = release('1.0.0');
const beta = release('1.1.0-beta.2', true);
const feed = (latest, recent, status = 200) => async url => url.endsWith('/latest')
  ? latest ? Response.json(latest) : new Response(null, { status: 404 })
  : Response.json(recent, { status });

function withModels(release) {
  const asset = release.assets[0];
  return { ...release, assets: [
    { ...asset, name: asset.name.replace('.dmg', '-with-models.dmg'), size: 166000000,
      browser_download_url: asset.browser_download_url.replace('.dmg', '-with-models.dmg') },
    ...release.assets
  ] };
}

test('model variant is explicit, independent of asset order, and never silently downgraded', async () => {
  const both = withModels(stable);
  assert.equal(installer(both).url, stable.assets[0].browser_download_url);
  assert.equal(installer(both).bundled.url, both.assets[0].browser_download_url);
  assert.equal(installer(stable).bundled, null);
  assert.equal(installer({ ...both, assets: [both.assets[0]] }), null);
  assert.equal(installer({ ...both, assets: [{ ...both.assets[0], browser_download_url: 'https://evil.example/model.dmg' }, ...stable.assets] }).bundled, null);
  for (const [path, latest, recent, expected] of [
    ['/download', both, [], stable.assets[0].browser_download_url],
    ['/download?variant=bundled', both, [], both.assets[0].browser_download_url],
    ['/download?variant=bundled&channel=beta', stable, [withModels(beta)], withModels(beta).assets[0].browser_download_url],
    ['/download?variant=bundled', null, [withModels(beta)], withModels(beta).assets[0].browser_download_url],
    ['/download?variant=bundled', stable, [withModels(beta)], RELEASES],
    ['/download?variant=unknown', both, [], RELEASES],
    ['/download?variant=bundled&asset=checksum', both, [], stable.assets[1].browser_download_url]
  ]) {
    const response = await handleReleaseRequest(new Request(`https://mixless.alkinum.com${path}`), feed(latest, recent));
    assert.equal(response.headers.get('location'), expected, path);
  }
});

test('stable remains preferred over a newer beta; stable is fetched independently of pagination', async () => {
  const data = await loadReleases(feed(stable, [beta]));
  assert.equal(data.recommended.version, 'v1.0.0');
  assert.equal(data.beta.version, 'v1.1.0-beta.2');
});
test('no stable: latest published beta with a complete installer wins; drafts and alpha are excluded', async () => {
  const draft = { ...release('2.0.0-beta.1', true, '2026-09-21T00:00:00Z'), draft: true };
  const data = await loadReleases(feed(null, [release('0.9.0-beta.1', true, '2026-01-01T00:00:00Z'), draft, release('3.0.0-alpha.1', true), beta]));
  assert.equal(data.stable, null);
  assert.equal(data.recommended.version, beta.tag_name);
});
test('API failure or incomplete stable cannot be misclassified as no stable', async () => {
  await assert.rejects(loadReleases(async () => new Response(null, { status: 403 })));
  await assert.rejects(loadReleases(feed({ ...stable, assets: [] }, [beta])));
});
test('invalid, foreign, empty, and draft assets never become download destinations', () => {
  assert.equal(installer({ ...beta, draft: true }), null);
  assert.equal(installer({ ...beta, assets: [] }), null);
  assert.equal(installer({ ...beta, assets: [{ ...beta.assets[0], browser_download_url: 'https://evil.example/file.dmg' }] }), null);
  assert.equal(installer({ ...beta, assets: [{ ...beta.assets[0], size: 0 }] }), null);
});
test('direct stable, beta, and checksum redirects work without JavaScript', async () => {
  for (const [path, destination] of [
    ['/download', stable.assets[0].browser_download_url],
    ['/download?channel=beta', beta.assets[0].browser_download_url],
    ['/download?asset=checksum', stable.assets[1].browser_download_url]
  ]) {
    const response = await handleReleaseRequest(new Request(`https://mixless.alkinum.com${path}`), feed(stable, [beta]));
    assert.equal(response.status, 302);
    assert.equal(response.headers.get('location'), destination);
    assert.equal(response.headers.get('cache-control'), 'no-store');
  }
});
test('beta fallback, no releases, unavailable channel, and network failure have usable destinations', async () => {
  const request = path => new Request(`https://mixless.alkinum.com${path}`);
  assert.equal((await handleReleaseRequest(request('/download'), feed(null, [beta]))).headers.get('location'), beta.assets[0].browser_download_url);
  for (const fetcher of [feed(null, []), async () => { throw new Error('offline'); }]) {
    assert.equal((await handleReleaseRequest(request('/download'), fetcher)).headers.get('location'), RELEASES);
  }
  assert.equal((await handleReleaseRequest(request('/download?channel=stable'), feed(null, [beta]))).headers.get('location'), RELEASES);
  assert.equal((await handleReleaseRequest(request('/api/releases'), async () => new Response(null, { status: 429 }))).status, 503);
});

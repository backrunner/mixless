export const REPOSITORY = 'https://github.com/backrunner/mixless';
export const RELEASES = `${REPOSITORY}/releases`;
const API = 'https://api.github.com/repos/backrunner/mixless/releases';

function releaseUrl(value, prefix) {
  return typeof value === 'string' && value.startsWith(prefix) ? value : null;
}

export function installer(release) {
  if (!release || release.draft || !Array.isArray(release.assets)) return null;
  const asset = release.assets.find(asset =>
    /^mixless-macos-.+\.dmg$/i.test(asset.name) && asset.size > 0 &&
    releaseUrl(asset.browser_download_url, `${RELEASES}/download/`));
  if (!asset) return null;
  const checksum = release.assets.find(asset => asset.name === 'SHA256SUMS.txt');
  return {
    version: release.tag_name,
    channel: release.prerelease ? 'beta' : 'stable',
    url: asset.browser_download_url,
    size: asset.size,
    notes: releaseUrl(release.html_url, `${RELEASES}/tag/`) ?? RELEASES,
    checksum: releaseUrl(checksum?.browser_download_url, `${RELEASES}/download/`),
    published: release.published_at
  };
}

export async function loadReleases(fetcher = fetch) {
  const get = async (url, allowMissing = false) => {
    const response = await fetcher(url, {
      headers: { Accept: 'application/vnd.github+json', 'User-Agent': 'mixless-site', 'X-GitHub-Api-Version': '2022-11-28' },
      signal: AbortSignal.timeout(8000),
      cf: { cacheTtl: 300, cacheEverything: true }
    });
    if (allowMissing && response.status === 404) return null;
    if (!response.ok) throw new Error(`Release service returned ${response.status}`);
    return response.json();
  };
  // /latest finds stable even if it is older than a page full of beta releases.
  const [stableRelease, recent] = await Promise.all([get(`${API}/latest`, true), get(`${API}?per_page=100`)]);
  if (!Array.isArray(recent)) throw new Error('Invalid release list');
  const stable = installer(stableRelease);
  // A published stable without an installer is incomplete, not permission to offer beta.
  if (stableRelease && (!stable || stableRelease.prerelease)) throw new Error('Stable installer unavailable');
  const candidates = recent.filter(release => release.prerelease && /-beta(?:[.\d-]|$)/i.test(release.tag_name));
  candidates.sort((a, b) => Date.parse(b.published_at) - Date.parse(a.published_at));
  const beta = candidates.map(installer).find(Boolean) ?? null;
  return { stable, beta, recommended: stable ?? beta };
}

export async function handleReleaseRequest(request, fetcher = fetch) {
  const url = new URL(request.url);
  url.pathname = url.pathname.replace(/\/$/, '');
  if (!['GET', 'HEAD'].includes(request.method)) return new Response(null, { status: 405, headers: { Allow: 'GET, HEAD' } });
  try {
    const data = await loadReleases(fetcher);
    if (url.pathname === '/api/releases') {
      return new Response(request.method === 'HEAD' ? null : JSON.stringify(data), {
        headers: { 'Content-Type': 'application/json', 'Cache-Control': 'public, max-age=60' }
      });
    }
    const channel = url.searchParams.get('channel');
    const release = channel === 'beta' ? data.beta : channel === 'stable' ? data.stable : data.recommended;
    const destination = url.searchParams.get('asset') === 'checksum' ? release?.checksum : release?.url;
    return new Response(null, { status: 302, headers: { Location: destination ?? RELEASES, 'Cache-Control': 'no-store' } });
  } catch (error) {
    console.warn('mixless release lookup failed:', error instanceof Error ? error.message : 'Unknown error');
    if (url.pathname === '/api/releases') return Response.json({ error: 'releases_unavailable' }, { status: 503, headers: { 'Cache-Control': 'no-store' } });
    // A usable, honest fallback when GitHub is unavailable or rate limited.
    return new Response(null, { status: 302, headers: { Location: RELEASES, 'Cache-Control': 'no-store' } });
  }
}

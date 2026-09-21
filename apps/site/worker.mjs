import { handleReleaseRequest } from './server/releases.mjs';

export default {
  async fetch(request, env) {
    const path = new URL(request.url).pathname.replace(/\/$/, '');
    if (path === '/download' || path === '/api/releases') return handleReleaseRequest(request);
    return env.ASSETS.fetch(request);
  }
};

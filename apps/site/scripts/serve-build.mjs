// A local-only static host for verifying the actual deployment artifact.
import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { extname, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { handleReleaseRequest } from '../server/releases.mjs';

const root = fileURLToPath(new URL('../build/', import.meta.url));
const mime = {
  '.html': 'text/html; charset=utf-8', '.js': 'text/javascript',
  '.css': 'text/css', '.json': 'application/json', '.png': 'image/png',
  '.webp': 'image/webp', '.svg': 'image/svg+xml', '.woff': 'font/woff',
  '.woff2': 'font/woff2', '.ttf': 'font/ttf', '.xml': 'application/xml',
  '.txt': 'text/plain; charset=utf-8', '.md': 'text/markdown; charset=utf-8'
};
await stat(resolve(root, 'index.html'));
const server = createServer(async (request, response) => {
  try {
    const path = decodeURIComponent(new URL(request.url, 'http://localhost').pathname);
    if (['/download', '/download/', '/api/releases', '/api/releases/'].includes(path)) {
      const result = await handleReleaseRequest(new Request(`http://localhost${request.url}`, { method: request.method }));
      response.writeHead(result.status, Object.fromEntries(result.headers));
      response.end(Buffer.from(await result.arrayBuffer()));
      return;
    }
    let file = resolve(root, `.${path}`);
    if (file !== resolve(root) && !file.startsWith(resolve(root) + sep)) {
      response.writeHead(400).end();
      return;
    }
    let status = 200;
    try {
      if ((await stat(file)).isDirectory()) file = resolve(file, 'index.html');
      await stat(file);
    } catch {
      status = 404;
      file = resolve(root, '404.html');
    }
    const data = await readFile(file);
    response.writeHead(status, {
      'Content-Type': mime[extname(file)] ?? 'application/octet-stream',
      'Cache-Control': 'no-store'
    });
    response.end(request.method === 'HEAD' ? undefined : data);
  } catch {
    response.writeHead(400).end();
  }
});
server.listen(Number(process.argv[2] ?? 4173), '127.0.0.1', () => {
  console.log(`mixless static preview: http://127.0.0.1:${server.address().port}`);
});

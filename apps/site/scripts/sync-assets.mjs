import sharp from 'sharp';
import { mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const root = new URL('../../../', import.meta.url);
const output = new URL('../static/', import.meta.url);
await Promise.all(['brand', 'images'].map(dir => mkdir(new URL(dir, output), { recursive: true })));
await Promise.all([
  sharp(fileURLToPath(new URL('assets/branding/app-icon-1024.png', root))).resize(128, 128).png().toFile(fileURLToPath(new URL('brand/icon.png', output))),
  sharp(fileURLToPath(new URL('assets/screenshots/workspace.png', root))).webp({ quality: 90 }).toFile(fileURLToPath(new URL('images/workspace.webp', output))),
  sharp(fileURLToPath(new URL('assets/screenshots/preferences.png', root))).resize(720).webp({ quality: 85 }).toFile(fileURLToPath(new URL('images/preferences.webp', output)))
]);
const backdrop = Buffer.from(`<svg width="1200" height="630"><rect width="1200" height="630" fill="#0c0c0e"/><text x="68" y="85" fill="#ffad66" font-family="Helvetica,Arial,sans-serif" font-weight="700" font-size="34">mixless.</text><text x="68" y="184" fill="#f5f3ee" font-family="Helvetica,Arial,sans-serif" font-weight="700" font-size="57">Your tracks. One continuous flow.</text><text x="70" y="235" fill="#a7a5a3" font-family="Helvetica,Arial,sans-serif" font-size="24">Local AI mixing. Native to Mac.</text></svg>`);
const preview = await sharp(fileURLToPath(new URL('assets/screenshots/workspace.png', root))).resize(1060).extract({ left: 0, top: 0, width: 1060, height: 345 }).toBuffer();
await sharp(backdrop).composite([{ input: preview, left: 70, top: 285 }]).png().toFile(fileURLToPath(new URL('images/social.png', output)));
console.log('Updated website assets from repository screenshots and branding.');

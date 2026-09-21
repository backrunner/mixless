import sharp from 'sharp';
import { mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const root = new URL('../../../', import.meta.url);
const output = new URL('../static/', import.meta.url);
const source = name => fileURLToPath(new URL(`assets/screenshots/${name}.png`, root));
const image = name => fileURLToPath(new URL(`images/${name}`, output));
const iconSource = fileURLToPath(new URL('assets/branding/app-icon-1024.png', root));
await Promise.all(['brand', 'images'].map(dir => mkdir(new URL(dir, output), { recursive: true })));
// Tab icons need real transparent corners; CSS rounding cannot shape a favicon.
const roundedIcon = await sharp(iconSource).composite([{
  input: Buffer.from('<svg width="1024" height="1024"><rect width="1024" height="1024" rx="240" fill="white"/></svg>'),
  blend: 'dest-in'
}]).png().toBuffer();
await Promise.all([32, 64].map(async size => {
  const padding = size / 32;
  await sharp(roundedIcon).resize(size - padding * 2).extend({
    top: padding, bottom: padding, left: padding, right: padding, background: '#00000000'
  }).png().toFile(fileURLToPath(new URL(`brand/favicon-${size}.png`, output)));
}));
// GitHub sanitizes inline styles, so README rounding must be part of the asset.
await sharp(roundedIcon).resize(320).extend({
  top: 8, bottom: 8, left: 8, right: 8, background: '#00000000'
}).png().toFile(fileURLToPath(new URL('assets/branding/readme-icon.png', root)));
const workspace = await sharp(source('workspace')).metadata();
if (workspace.width < 2560) throw new Error('Capture a native high-resolution workspace; do not upscale a preview.');
await Promise.all([
  sharp(source('workspace')).composite([{
    input: Buffer.from(`<svg width="${workspace.width}" height="${workspace.height}"><rect width="${workspace.width}" height="${workspace.height}" rx="32" fill="white"/></svg>`),
    blend: 'dest-in'
  }]).webp({ quality: 94 }).toFile(fileURLToPath(new URL('assets/screenshots/workspace-readme.webp', root))),
  sharp(iconSource).resize(256, 256).png().toFile(fileURLToPath(new URL('brand/icon.png', output))),
  sharp(source('workspace')).webp({ quality: 94 }).toFile(image('workspace-full.webp')),
  sharp(source('workspace')).resize(1680).webp({ quality: 91 }).toFile(image('workspace.webp')),
  sharp(source('workspace')).resize(840).webp({ quality: 86 }).toFile(image('workspace-small.webp')),
  sharp(source('preferences')).webp({ quality: 92 }).toFile(image('preferences.webp'))
]);
// Keep intrinsic sizes and srcset descriptors tied to the actual capture.
await writeFile(new URL('../src/lib/landing/screenshot.json', import.meta.url), JSON.stringify({
  width: workspace.width, height: workspace.height,
  displayWidth: 1680, displayHeight: Math.round(workspace.height * 1680 / workspace.width)
}, null, 2) + '\n');
const backdrop = Buffer.from(`<svg width="1200" height="630"><defs><linearGradient id="a"><stop stop-color="#ffe3a9"/><stop offset="1" stop-color="#ffb224"/></linearGradient></defs><rect width="1200" height="630" fill="#0c0c0e"/><text x="114" y="78" fill="#f0ede6" font-family="Helvetica Neue,Helvetica,Arial,sans-serif" font-weight="600" letter-spacing="-.85" font-size="34">mixless<tspan fill="#f5d90a">.</tspan></text><text x="60" y="167" fill="url(#a)" font-family="Helvetica Neue,Helvetica,Arial,sans-serif" font-weight="600" letter-spacing="-1.8" font-size="58">Your tracks. One continuous flow.</text><text x="63" y="217" fill="#aaa39a" font-family="Helvetica,Arial,sans-serif" font-size="23">Dual decks. Local stems. Musical AutoMix.</text></svg>`);
const preview = await sharp(source('workspace')).resize(1080).extract({ left: 0, top: 0, width: 1080, height: 358 }).toBuffer();
const icon = await sharp(fileURLToPath(new URL('brand/icon.png', output))).resize(38).toBuffer();
await sharp(backdrop).composite([{ input: icon, left: 62, top: 47 }, { input: preview, left: 60, top: 272 }]).png().toFile(image('social.png'));
console.log(`Updated site assets from native ${workspace.width} × ${workspace.height} capture.`);

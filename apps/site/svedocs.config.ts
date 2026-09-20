import { defineConfig } from 'svedocs/config';
import { en, zh } from './src/lib/messages.ts';

const url = process.env.SITE_URL ?? 'https://mixless.alkinum.com';

export default defineConfig({
  site: {
    name: 'Mixless', title: 'Mixless', url,
    description: 'A native Mac DJ workspace. Local AI analysis, dual decks, and musical AutoMix.'
  },
  build: { mode: 'static' },
  theme: {
    defaultMode: 'dark',
    palette: { accent: '#ffad66', neutral: '#0c0c0e' },
    fonts: {
      sans: '-apple-system, BlinkMacSystemFont, "Segoe UI", "Noto Sans SC", sans-serif',
      display: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
      mono: '"SFMono-Regular", Consolas, monospace'
    },
    radius: '10px',
    brand: { label: 'Mixless', href: '/', logo: '/brand/icon.png', mark: false },
    nav: [
      { label: 'The workspace', labelKey: 'site.workspace', href: '/#workspace' },
      { label: 'AutoMix', href: '/#automix' },
      { label: 'Guide', labelKey: 'site.guide', href: '/docs' }
    ],
    footer: { text: 'Mixless', links: [] }
  },
  images: false,
  search: { provider: 'local', scope: 'current' },
  ai: false,
  agent: { negotiation: false },
  source: { editBaseUrl: 'https://github.com/backrunner/mixless/edit/main/apps/site' },
  checks: { translations: true },
  i18n: {
    defaultLocale: 'en', prefixDefaultLocale: false,
    locales: [
      { code: 'en', label: 'English', path: 'en', hreflang: 'en', dir: 'ltr' },
      { code: 'zh', label: '中文', path: 'zh', hreflang: 'zh-CN', dir: 'ltr' }
    ],
    messages: { en, zh }
  },
  seo: {
    sitemap: true, robots: true, defaultAuthor: 'Alkinum', ogImage: false
  }
});

import type { Handle } from '@sveltejs/kit';
import { resolveSvedocsPageRoute } from 'svedocs/routes';
import config from 'virtual:svedocs/config';
import pages from 'virtual:svedocs/page-index';

// This also runs during prerendering: crawlers get the language before hydration.
export const handle: Handle = ({ event, resolve }) => {
  const route = resolveSvedocsPageRoute(event.url.pathname, pages, config);
  const code = route.status === 'missing' ? route.requestedLocale : route.page.locale;
  const locale = config.i18n.locales.find(item => item.code === (code ?? config.i18n.defaultLocale));
  const language = locale?.hreflang ?? locale?.code ?? 'en';
  return resolve(event, {
    transformPageChunk: ({ html }) => html
      .replaceAll('%site.lang%', language)
      .replaceAll('%site.dir%', locale?.dir ?? 'ltr')
  });
};

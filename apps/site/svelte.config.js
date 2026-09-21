import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';
import { svedocsPreprocess, svedocsSvelteExtensions } from 'svedocs/svelte';

export default {
  extensions: svedocsSvelteExtensions,
  preprocess: [vitePreprocess(), svedocsPreprocess()],
  kit: {
    adapter: adapter({ fallback: '404.html' }),
    prerender: {
      handleHttpError: ({ path, message }) => {
        // These URLs are served at request time by worker.mjs, outside SvelteKit.
        if (path === '/download' || path === '/download/') return;
        throw new Error(message);
      }
    }
  }
};

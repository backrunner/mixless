<script lang="ts">
  import { resolveLocalizedHref } from 'svedocs/theme/headless';
  import type { SvedocsFooterProps } from 'svedocs/theme/types';
  import Wordmark from '../brand/Wordmark.svelte';
  let { context }: SvedocsFooterProps = $props();
</script>

<footer class="mx-footer">
  <div class="mx-footer-top">
    <div><a class="mx-brand" href={resolveLocalizedHref('/', context)} aria-label="mixless"><Wordmark /></a></div>
    <nav aria-label={context.t('site.support')}>
      <a href={resolveLocalizedHref('/docs', context)}>{context.t('site.guide')}</a>
      <a href={resolveLocalizedHref('/#downloads', context)}>{context.t('download.other')}</a>
      <a href={resolveLocalizedHref('/privacy', context)}>{context.t('site.privacy')}</a>
      <a href={resolveLocalizedHref('/support', context)}>{context.t('site.support')}</a>
      <a href="https://github.com/backrunner/mixless">GitHub</a>
    </nav>
  </div>
  <div class="mx-footer-bottom"><span>© 2026 Alkinum</span><a href="https://github.com/backrunner/mixless/blob/main/LICENSE">{context.t('site.openSource')}</a></div>
</footer>

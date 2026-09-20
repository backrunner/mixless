<script lang="ts">
  import type { SvedocsThemeContext } from 'svedocs/theme/types';
  import { resolveLocalizedHref } from 'svedocs/theme/headless';
  import Transition from './Transition.svelte';
  import './landing.css';
  let { context }: { context: SvedocsThemeContext } = $props();
  const releases = 'https://github.com/backrunner/mixless/releases';
  const features = ['manual', 'stems', 'local'] as const;
  const guides = [{ key: 'start', href: '/docs' }, { key: 'audio', href: '/docs/audio' }, { key: 'midi', href: '/docs/midi' }];
</script>

<section class="mx-hero mx-container">
  <h1>{context.t('hero.first')}<br /><span>{context.t('hero.second')}</span></h1>
  <p class="mx-hero-description">{context.t('hero.description')}</p>
  <div class="mx-actions">
    <a class="mx-button mx-primary" href={releases}>{context.t('site.releases')}</a>
    <a class="mx-button mx-secondary" href={resolveLocalizedHref('/docs', context)}>{context.t('hero.guide')}</a>
  </div>
</section>

<section id="workspace" class="mx-workspace mx-container" aria-label={context.t('site.workspace')}>
  <figure class="mx-product">
    <img src="/images/workspace.webp" width="1680" height="1050" alt={context.t('workspace.alt')} fetchpriority="high" />
  </figure>
  <div class="mx-feature-row">
    {#each features as feature}
      <div class="mx-feature">
        <h2>{context.t(`workspace.${feature}`)}</h2>
        <p>{context.t(`workspace.${feature}Text`)}</p>
      </div>
    {/each}
  </div>
</section>

<section id="automix" class="mx-automix mx-container">
  <div class="mx-automix-copy">
    <h2>{context.t('automix.first')}<br />{context.t('automix.second')}</h2>
    <p>{context.t('automix.description')}</p>
    <a class="mx-text-link" href={resolveLocalizedHref('/docs/automix', context)}>{context.t('automix.link')}</a>
  </div>
  <Transition {context} />
</section>

<section class="mx-guide mx-container">
  <h2>{context.t('guide.title')}</h2>
  <div class="mx-guide-links">
    {#each guides as guide}
      <a href={resolveLocalizedHref(guide.href, context)}>
        <div><h3>{context.t(`guide.${guide.key}`)}</h3><p>{context.t(`guide.${guide.key}Text`)}</p></div>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12h14m-6-6 6 6-6 6" /></svg>
      </a>
    {/each}
  </div>
</section>

<section class="mx-closing mx-container">
  <h2>{context.t('closing.title')}</h2>
  <a class="mx-button mx-primary" href={releases}>{context.t('site.releases')}</a>
</section>

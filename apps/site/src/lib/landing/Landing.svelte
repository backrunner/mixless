<script lang="ts">
  import { onMount } from 'svelte';
  import type { SvedocsThemeContext } from 'svedocs/theme/types';
  import { resolveLocalizedHref } from 'svedocs/theme/headless';
  import Transition from './Transition.svelte';
  import Wordmark from '../brand/Wordmark.svelte';
  import screenshot from './screenshot.json';
  import './landing.css';
  let { context }: { context: SvedocsThemeContext } = $props();
  const releases = 'https://github.com/backrunner/mixless/releases';
  const features = ['manual', 'stems', 'local'] as const;
  const guides = [{ key: 'start', href: '/docs' }, { key: 'audio', href: '/docs/audio' }, { key: 'midi', href: '/docs/midi' }];
  type Release = { version: string; channel: 'stable' | 'beta'; size: number; notes: string; checksum: string | null };
  let release = $state<Release | null>(null);
  let beta = $state<Release | null>(null);
  let status = $state<'loading' | 'ready' | 'unavailable'>('loading');
  let motion = $state(true);
  onMount(() => {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 10000);
    fetch('/api/releases', { signal: controller.signal })
      .then(response => { if (!response.ok) throw new Error('Unavailable'); return response.json(); })
      .then(data => { release = data.recommended; beta = data.beta; status = release ? 'ready' : 'unavailable'; })
      .catch(() => { status = 'unavailable'; })
      .finally(() => clearTimeout(timeout));
    return () => { controller.abort(); clearTimeout(timeout); };
  });
  function reveal(node: HTMLElement) {
    if (!('IntersectionObserver' in window)) return;
    const observer = new IntersectionObserver(entries => {
      for (const entry of entries) if (entry.isIntersecting) { node.classList.add('mx-enter'); observer.disconnect(); }
    }, { threshold: .12 });
    observer.observe(node);
    return { destroy: () => observer.disconnect() };
  }
</script>

<div class="mx-landing" data-motion={motion ? 'playing' : 'paused'}>
  <section class="mx-hero">
    <svg class="mx-flow-art" viewBox="0 0 1440 650" fill="none" aria-hidden="true">
      <defs><linearGradient id="flow-color" x1="0" y1="0" x2="1440" y2="0" gradientUnits="userSpaceOnUse"><stop stop-color="#ff5238" /><stop offset=".5" stop-color="#ffb224" /><stop offset="1" stop-color="#f5d90a" /></linearGradient></defs>
      <path class="mx-flow-line" pathLength="1" d="M-80 470H90C155 470 150 145 226 145S295 560 367 560S425 345 500 345H940C1015 345 1005 135 1080 135S1145 540 1220 540S1290 250 1355 250H1520" />
      <path class="mx-flow-echo" d="M-80 490H90C155 490 150 165 226 165S295 580 367 580S425 365 500 365H940C1015 365 1005 155 1080 155S1145 560 1220 560S1290 270 1355 270H1520" />
    </svg>
    <div class="mx-hero-content mx-container">
      <div class="mx-kicker"><span class="mx-signal" aria-hidden="true">{#each [0, 1, 2, 3, 4] as i}<i style={`--i:${i}`}></i>{/each}</span>{context.t('hero.kicker')}</div>
      <h1>{context.t('hero.first')}<br /><span>{context.t('hero.second')}</span></h1>
      <p class="mx-hero-description">{context.t('hero.description')}</p>
      <div class="mx-actions">
        <a class="mx-button mx-primary" href="/download" data-sveltekit-reload><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3v12m-5-5 5 5 5-5M5 16v4h14v-4" /></svg>{context.t('site.releases')}</a>
        <a class="mx-button mx-secondary" href={resolveLocalizedHref('/docs', context)}>{context.t('hero.guide')} <span aria-hidden="true">↗</span></a>
      </div>
      <div class="mx-download-caption"><span>{release ? `${release.version} · ${context.t(`download.${release.channel}`)}` : context.t('download.platform')}</span><span aria-hidden="true">/</span><a href="#downloads">{context.t('download.other')}</a></div>
    </div>
  </section>

  <section id="workspace" class="mx-workspace mx-container" aria-label={context.t('site.workspace')}>
    <div class="mx-stage-label"><span><i aria-hidden="true"></i>{context.t('workspace.caption')}</span><button class="mx-motion-toggle" type="button" aria-pressed={!motion} onclick={() => motion = !motion}>{context.t(motion ? 'motion.pause' : 'motion.play')}</button></div>
    <figure class="mx-product">
      <a href="/images/workspace-full.webp" target="_blank" rel="noreferrer" aria-label={context.t('workspace.expand')}>
        <img src="/images/workspace.webp" srcset={`/images/workspace-small.webp 840w, /images/workspace.webp 1680w, /images/workspace-full.webp ${screenshot.width}w`} sizes="(max-width: 700px) calc(100vw - 32px), (max-width: 1360px) calc(100vw - 80px), 1280px" width={screenshot.displayWidth} height={screenshot.displayHeight} alt={context.t('workspace.alt')} fetchpriority="high" />
        <span class="mx-image-expand" aria-hidden="true">↗</span>
      </a>
    </figure>
    <div class="mx-feature-row" use:reveal>
      {#each features as feature, i}
        <div class="mx-feature"><span class="mx-feature-number" aria-hidden="true">0{i + 1} /</span><h2>{context.t(`workspace.${feature}`)}</h2><p>{context.t(`workspace.${feature}Text`)}</p></div>
      {/each}
    </div>
  </section>

  <section id="automix" class="mx-automix mx-container" use:reveal>
    <div class="mx-automix-copy"><p class="mx-eyebrow">AutoMix</p><h2>{context.t('automix.first')}<br />{context.t('automix.second')}</h2><p>{context.t('automix.description')}</p><a class="mx-text-link" href={resolveLocalizedHref('/docs/automix', context)}>{context.t('automix.link')} <span aria-hidden="true">↗</span></a></div>
    <Transition {context} />
  </section>

  <section class="mx-guide mx-container" use:reveal>
    <div><p class="mx-eyebrow">{context.t('guide.eyebrow')}</p><h2>{context.t('guide.title')}</h2><p class="mx-guide-description">{context.t('guide.description')}</p></div>
    <div class="mx-guide-links">{#each guides as guide, i}<a href={resolveLocalizedHref(guide.href, context)}><span class="mx-guide-number" aria-hidden="true">0{i + 1}</span><div><h3>{context.t(`guide.${guide.key}`)}</h3><p>{context.t(`guide.${guide.key}Text`)}</p></div><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12h14m-6-6 6 6-6 6" /></svg></a>{/each}</div>
  </section>

  <section id="downloads" class="mx-download-section mx-container" use:reveal aria-labelledby="download-title">
    <div class="mx-download-intro"><img src="/brand/icon.png" width="80" height="80" alt="" loading="lazy" /><div><p class="mx-eyebrow">{context.t('download.eyebrow')}</p><h2 id="download-title">{context.t('closing.title')}</h2></div></div>
    <div class="mx-download-grid">
      <div class="mx-download-main">
        <div class="mx-download-heading"><Wordmark /><span class="mx-release-badge">{release ? context.t(`download.${release.channel}`) : 'macOS'}</span></div>
        <p class="mx-system">{context.t('download.platform')}<br />{context.t('download.arch')}</p>
        <a class="mx-button mx-primary" href="/download" data-sveltekit-reload>{context.t('site.releases')} <span aria-hidden="true">↓</span></a>
        <p class="mx-release-status" aria-live="polite">{#if release}{release.version} · {(release.size / 1024 / 1024).toFixed(1)} MB · DMG{:else}{context.t(status === 'loading' ? 'download.loading' : 'download.unavailable')}{/if}</p>
        <p class="mx-release-policy">{context.t(release?.channel === 'beta' ? 'download.betaNotice' : 'download.policy')}</p>
      </div>
      <div class="mx-download-options"><h3>{context.t('download.other')}</h3>
        {#if beta}<a href="/download?channel=beta" data-sveltekit-reload><span>{context.t('download.latestBeta')}<small>{beta.version}</small></span><span aria-hidden="true">↓</span></a>{/if}
        <a href={release?.notes ?? releases}><span>{context.t('download.notes')}</span><span aria-hidden="true">↗</span></a>
        <a href={releases}><span>{context.t('download.history')}</span><span aria-hidden="true">↗</span></a>
        {#if release?.checksum}<a href="/download?asset=checksum" data-sveltekit-reload><span>{context.t('download.checksum')}<small>SHA-256</small></span><span aria-hidden="true">↓</span></a>{/if}
        <a href="https://github.com/backrunner/mixless/archive/refs/heads/main.zip"><span>{context.t('download.source')}<small>ZIP · MPL 2.0</small></span><span aria-hidden="true">↓</span></a>
      </div>
    </div>
    <p class="mx-download-footnote">{context.t('download.macOnly')} <a href={resolveLocalizedHref('/docs/development', context)}>{context.t('download.build')}</a></p>
  </section>
</div>

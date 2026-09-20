<script lang="ts">
  import { MobileNav, SearchDialog } from 'svedocs/theme';
  import { resolveLocalizedHref } from 'svedocs/theme/headless';
  import type { SvedocsNavbarProps } from 'svedocs/theme/types';

  let { context, mobileTree = [], mobileCurrentPath = '', mobileMenuId = 'mixless-menu',
    mobileMenuOpen = false, onToggleMobileMenu, onCloseMobileMenu }: SvedocsNavbarProps = $props();
  let otherLanguage = $derived(context.localeCode === 'zh' ? 'en' : 'zh');
  let alternate = $derived(context.pages.find(p => p.scopePath === context.page?.scopePath && p.locale === otherLanguage));
</script>

<header class="mx-header" class:mx-menu-open={mobileMenuOpen}>
  <div class="mx-header-inner">
    <a class="mx-brand" href={resolveLocalizedHref('/', context)} aria-label="Mixless">
      <img src="/brand/icon.png" width="34" height="34" alt="" />
      <span>mixless<span class="mx-brand-dot" aria-hidden="true">.</span></span>
    </a>
    <nav class="mx-nav" aria-label={context.t('nav.primary')}>
      {#each context.config.theme.nav as item}
        <a href={resolveLocalizedHref(item.href, context)} aria-current={item.href === '/docs' && context.isDocsPage ? 'page' : undefined}>{item.labelKey ? context.t(item.labelKey) : item.label}</a>
      {/each}
    </nav>
    <div class="mx-header-tools">
      <SearchDialog records={context.search} loadRecords={context.loadSearch} scope={context.searchScope} provider="local" buildMode={context.config.build.mode} {context} />
      {#if alternate}
        <a class="mx-language" href={alternate.routePath} aria-label={context.t('site.language')} lang={otherLanguage === 'zh' ? 'zh-CN' : 'en'}>{otherLanguage === 'zh' ? '中' : 'EN'}</a>
      {/if}
      <a class="mx-header-download" href="https://github.com/backrunner/mixless/releases">{context.t('site.download')}</a>
      <button class="mx-menu-button" type="button" onclick={onToggleMobileMenu} aria-expanded={mobileMenuOpen} aria-controls={mobileMenuId} aria-label={context.t(mobileMenuOpen ? 'nav.mobile.close' : 'nav.mobile.open')}>
        {#if mobileMenuOpen}<span aria-hidden="true">×</span>{:else}<svg aria-hidden="true" viewBox="0 0 24 24"><path d="M4 8h16M4 16h16" /></svg>{/if}
      </button>
    </div>
  </div>
  <div class="mx-mobile-menu" id={mobileMenuId} hidden={!mobileMenuOpen}>
    <nav aria-label={context.t('nav.primary')}>
      {#each context.config.theme.nav as item}
        <a href={resolveLocalizedHref(item.href, context)} onclick={onCloseMobileMenu}>{item.labelKey ? context.t(item.labelKey) : item.label}</a>
      {/each}
      <a href="https://github.com/backrunner/mixless/releases">{context.t('site.download')}</a>
    </nav>
    <MobileNav items={mobileTree} currentPath={mobileCurrentPath} {context} />
  </div>
</header>

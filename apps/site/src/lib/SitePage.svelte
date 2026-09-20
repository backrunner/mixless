<script lang="ts">
  import { DocsApp } from 'svedocs/theme';
  import type { SvedocsAppProps } from 'svedocs/theme/types';
  import themeComponents from 'virtual:svedocs/theme-components';
  import loadSearch from 'virtual:svedocs/search-loader';
  import Landing from './landing/Landing.svelte';
  export let data: SvedocsAppProps;
</script>

<DocsApp {...data} {themeComponents} {loadSearch}>
  <svelte:fragment slot="landing" let:context>
    <Landing {context} />
  </svelte:fragment>
</DocsApp>

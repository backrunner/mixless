<script lang="ts">
  import type { SvedocsThemeContext } from 'svedocs/theme/types';
  let { context }: { context: SvedocsThemeContext } = $props();
  const modes = ['blend', 'bass', 'cut'] as const;
  type Mode = typeof modes[number];
  let mode = $state<Mode>('blend');
  const bars = Array.from({ length: 76 }, (_, i) => i);
  function height(i: number, incoming: boolean) {
    const position = i / 75;
    const pulse = 14 + Math.abs(Math.sin(i * 1.9 + (incoming ? 2 : 0)) * Math.cos(i * .38)) * 70;
    const envelope = mode === 'cut' ? (incoming ? Number(position >= .52) : Number(position < .52))
      : mode === 'bass' ? (incoming ? .2 + position * .8 : 1 - position * .8)
      : incoming ? position : 1 - position;
    return Math.max(3, pulse * envelope);
  }
</script>

<div class="mx-transition" data-mode={mode}>
  <div class="mx-transition-tabs" role="group" aria-label={context.t('automix.demo')}>
    {#each modes as item}<button type="button" aria-pressed={mode === item} onclick={() => mode = item}>{context.t(`automix.${item}`)}</button>{/each}
  </div>
  <div class="mx-transition-plot" aria-hidden="true">
    <div class="mx-playhead"><span></span></div>
    <div class="mx-track-label"><b>A</b><span>{context.t('automix.outgoing')}</span></div>
    <div class="mx-track mx-track-a">{#each bars as i}<i style={`height:${height(i, false)}%`}></i>{/each}</div>
    <div class="mx-track-label"><b>B</b><span>{context.t('automix.incoming')}</span></div>
    <div class="mx-track mx-track-b">{#each bars as i}<i style={`height:${height(i, true)}%`}></i>{/each}</div>
  </div>
  <div class="mx-transition-description" aria-live="polite"><p>{context.t(`automix.${mode}Text`)}</p></div>
  <p class="mx-diagram-note">{context.t('automix.diagram')}</p>
</div>

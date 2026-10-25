<script lang="ts">
  export let name:
    | 'clock'
    | 'pause'
    | 'play'
    | 'history'
    | 'keyboard'
    | 'palette'
    | 'refresh'
    | 'database'
    | 'arrow'
    | 'close'
    | 'edit'
    | 'trash'
    | 'back'
    | 'check'
    | 'window'
    | 'download'
    | 'upload';
  const paths = {
    clock: 'M12 8v4l3 2 M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0Z',
    pause: 'M8 5v14M16 5v14',
    play: 'm8 5 11 7-11 7V5Z',
    history: 'M3 11a9 9 0 1 1 2.6 7.4M3 4v7h7M12 7v5l3 2',
    keyboard:
      'M4 5h16a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2ZM6 9h.01M10 9h.01M14 9h.01M18 9h.01M6 13h.01M10 13h.01M14 13h.01M18 13h.01M8 16h8',
    palette:
      'M12 3a9 9 0 1 0 0 18h1a2 2 0 0 0 0-4 2 2 0 0 1 0-4h4a4 4 0 0 0 4-4c0-3.3-4-6-9-6ZM7 10h.01M9 6.5h.01M14 6.5h.01M17 9h.01',
    refresh: 'M20 7a9 9 0 0 0-15-2L2 8m0-5v5h5m-3 9a9 9 0 0 0 15 2l3-3m0 5v-5h-5',
    database:
      'M21 6c0 2.2-4 4-9 4s-9-1.8-9-4 4-4 9-4 9 1.8 9 4ZM3 6v12c0 2.2 4 4 9 4s9-1.8 9-4V6M3 12c0 2.2 4 4 9 4s9-1.8 9-4',
    arrow: 'm9 5 7 7-7 7',
    close: 'm6 6 12 12M6 18 18 6',
    edit: 'm16 3 5 5-12 12-6 1 1-6L16 3Zm-3 3 5 5',
    trash: 'M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7',
    back: 'm12 5-7 7 7 7M5 12h15',
    check: 'm5 12 4 4L19 6',
    window: 'M3 8V3h14M7 7h14v14H7V7Z',
    download: 'M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5',
    upload: 'M12 16V4m-5 5 5-5 5 5M4 16v5h16v-5',
  };
</script>

<svg
  class="settings-icon"
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.7"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"><path d={paths[name]} /></svg
>

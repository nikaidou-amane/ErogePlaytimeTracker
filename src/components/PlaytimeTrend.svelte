<script lang="ts">
  import { onMount } from 'svelte';
  import {
    advanceChartInertia,
    chartAxisTicks,
    chartPanScrollLeft,
    compactDuration,
    dailyTrend,
    formatDateKey,
    nextChartZoom,
    timestampMarkerGroups,
    type DailyTrendPoint,
    type TimestampMarkerGroup,
  } from '../lib/statistics';
  import { imageSrc } from '../lib/time';
  import type { GameTimestamp, StatisticsDay, StatisticsPeriodKind } from '../lib/types';

  export let days: StatisticsDay[] = [];
  export let kind: StatisticsPeriodKind;
  export let timestamps: GameTimestamp[] = [];
  export let expanded = false;
  export let fixedRange = false;
  export let showGameBreakdown = true;

  const width = 900;
  const panThreshold = 4;
  const gameColors = [
    '#9b7be8',
    '#d06f8d',
    '#df9248',
    '#4da3cf',
    '#7cab55',
    '#b96bc2',
    '#d0ad3d',
    '#607fd0',
    '#cb685e',
    '#4a9b83',
  ];
  let chartContainerElement: HTMLDivElement;
  let chartElement: HTMLDivElement;
  let chartSvgElement: SVGSVGElement;
  let viewportElement: HTMLDivElement;
  let tooltipElement: HTMLDivElement;
  let zoom = 1;
  let hoverIndex: number | null = null;
  let pinnedIndex: number | null = null;
  let activeIndex: number | null = null;
  let currentDays = days;
  let chartPixelHeight = 180;
  let chartPixelScale = 1;
  let chartPixelOffset = 0;
  let tooltipLeft = 0;
  let tooltipAlignStart = false;
  let tooltipAlignEnd = false;
  let panPointerId: number | null = null;
  let panStartX = 0;
  let panStartScrollLeft = 0;
  let panLastScrollLeft = 0;
  let panLastTime = 0;
  let panVelocity = 0;
  let panning = false;
  let suppressClickUntil = 0;
  let inertiaFrame: number | null = null;

  $: points = dailyTrend(days);
  $: timestampGroups = timestampMarkerGroups(timestamps);
  $: chartViewHeight = expanded
    ? Math.max(240, Math.min(420, timestampGroups.length * 15 + 100))
    : 180;
  $: plotTop = 14;
  $: plotBottom = chartViewHeight - 40;
  $: plotHeight = plotBottom - plotTop;
  $: if (days !== currentDays) {
    stopInertia();
    resetPan();
    currentDays = days;
    hoverIndex = null;
    pinnedIndex = null;
    zoom = 1;
    if (viewportElement) viewportElement.scrollLeft = 0;
  }
  $: activeIndex = pinnedIndex ?? hoverIndex;
  $: maximumZoom = Math.min(32, Math.max(4, points.length / 45));
  $: if (zoom > maximumZoom) zoom = maximumZoom;
  $: chartWidth = width * zoom;
  $: dailyAxisTicks = chartAxisTicks(Math.max(0, ...points.map((point) => point.playtime_seconds)));
  $: cumulativeAxisTicks = chartAxisTicks(
    Math.max(
      0,
      ...points.map((point) => point.cumulative_seconds),
      ...timestampGroups.map((marker) => marker.playtimeSeconds),
    ),
  );
  $: maximumDaily = dailyAxisTicks[dailyAxisTicks.length - 1].value;
  $: maximumCumulative = cumulativeAxisTicks[cumulativeAxisTicks.length - 1].value;
  $: labelStep = Math.max(1, Math.ceil(points.length / (8 * zoom)));
  $: barStrokeWidth = Math.max(
    0.45,
    Math.min(18, (chartWidth / Math.max(1, points.length)) * 0.68),
  );
  $: stackedBars = buildStackedBars(points, maximumDaily, chartWidth);
  $: cumulativePoints = buildCumulativePoints(points, maximumCumulative, chartWidth);
  $: timestampLines = buildTimestampLines(timestampGroups, maximumCumulative, plotTop, plotBottom);
  $: activePoint = activeIndex === null ? null : (points[activeIndex] ?? null);
  $: activeGames = activePoint?.games ?? [];
  $: accessibleIndex = activeIndex ?? Math.max(0, points.length - 1);
  $: accessiblePoint = points[accessibleIndex];
  $: accessibleValue = accessiblePoint
    ? `${formatDateKey(accessiblePoint.date)}、合計${compactDuration(accessiblePoint.playtime_seconds)}、累計${compactDuration(accessiblePoint.cumulative_seconds)}`
    : 'データなし';
  $: if (chartSvgElement && chartWidth) requestAnimationFrame(syncAxisLayout);
  $: if (activeIndex !== null && chartContainerElement && viewportElement && chartWidth) {
    requestAnimationFrame(syncTooltipPosition);
  }

  onMount(() => {
    function handleDocumentClick(event: MouseEvent) {
      if (pinnedIndex === null || !(event.target instanceof Node)) return;
      if (tooltipElement?.contains(event.target) || chartElement?.contains(event.target)) return;
      pinnedIndex = null;
      hoverIndex = null;
    }

    document.addEventListener('click', handleDocumentClick);
    viewportElement.addEventListener('wheel', handleWheel, { passive: false });
    viewportElement.addEventListener('scroll', syncTooltipPosition);
    const resizeObserver = new ResizeObserver(syncAxisLayout);
    resizeObserver.observe(chartSvgElement);
    syncAxisLayout();
    return () => {
      document.removeEventListener('click', handleDocumentClick);
      viewportElement.removeEventListener('wheel', handleWheel);
      viewportElement.removeEventListener('scroll', syncTooltipPosition);
      resizeObserver.disconnect();
      stopInertia();
      resetPan();
    };
  });

  function syncAxisLayout() {
    if (!chartSvgElement) return;
    const bounds = chartSvgElement.getBoundingClientRect();
    if (!bounds.width || !bounds.height) return;
    const scale = Math.min(bounds.width / chartWidth, bounds.height / chartViewHeight);
    chartPixelHeight = bounds.height;
    chartPixelScale = scale;
    chartPixelOffset = (bounds.height - chartViewHeight * scale) / 2;
    syncTooltipPosition();
  }

  function syncTooltipPosition() {
    if (activeIndex === null || !points.length || !chartContainerElement || !viewportElement)
      return;

    const chartBounds = chartContainerElement.getBoundingClientRect();
    const viewportBounds = viewportElement.getBoundingClientRect();
    const pointRatio = (activeIndex + 0.5) / points.length;
    const pointLeft =
      viewportBounds.left -
      chartBounds.left +
      pointRatio * viewportElement.scrollWidth -
      viewportElement.scrollLeft;
    const tooltipWidth = tooltipElement?.offsetWidth ?? Math.min(310, chartBounds.width - 8);
    const edgeInset = 4;

    tooltipAlignStart = pointLeft - tooltipWidth / 2 < edgeInset;
    tooltipAlignEnd =
      !tooltipAlignStart && pointLeft + tooltipWidth / 2 > chartBounds.width - edgeInset;
    tooltipLeft = tooltipAlignStart
      ? edgeInset
      : tooltipAlignEnd
        ? chartBounds.width - edgeInset
        : pointLeft;
  }

  function axisTickTop(ratio: number) {
    return chartPixelOffset + (plotBottom - ratio * plotHeight) * chartPixelScale;
  }

  function axisTitleTop() {
    return chartPixelOffset + (chartViewHeight - 16) * chartPixelScale;
  }

  function pointX(index: number, pointCount: number, graphWidth: number) {
    return ((index + 0.5) / Math.max(1, pointCount)) * graphWidth;
  }

  function gameColor(gameId: number) {
    return gameColors[Math.abs(Math.trunc(gameId)) % gameColors.length];
  }

  function buildStackedBars(points: DailyTrendPoint[], maximum: number, graphWidth: number) {
    const bars: Array<{ x: number; top: number; bottom: number; color: string }> = [];
    points.forEach((point, index) => {
      let stackedSeconds = 0;
      point.games.forEach((game) => {
        const bottom = plotBottom - (stackedSeconds / maximum) * plotHeight;
        stackedSeconds += game.playtime_seconds;
        bars.push({
          x: pointX(index, points.length, graphWidth),
          top: plotBottom - (stackedSeconds / maximum) * plotHeight,
          bottom,
          color: gameColor(game.game_id),
        });
      });
    });
    return bars;
  }

  function buildCumulativePoints(points: DailyTrendPoint[], maximum: number, graphWidth: number) {
    return points
      .map((point, index) => {
        const x = pointX(index, points.length, graphWidth);
        const y = plotBottom - (point.cumulative_seconds / maximum) * plotHeight;
        return `${x},${y}`;
      })
      .join(' ');
  }

  function buildTimestampLines(
    groups: TimestampMarkerGroup[],
    maximum: number,
    top: number,
    bottom: number,
  ) {
    const minimumLabelY = top + 7;
    const maximumLabelY = bottom - 6;
    const labelGap = 14;
    const lines = groups.map((group) => {
      const y = bottom - (group.playtimeSeconds / maximum) * (bottom - top);
      return {
        ...group,
        y,
        labelY: Math.max(minimumLabelY, y - 4),
        label: `${group.names.join(' / ')} (${compactDuration(group.playtimeSeconds)})`,
      };
    });
    for (let index = 1; index < lines.length; index += 1) {
      lines[index].labelY = Math.max(lines[index].labelY, lines[index - 1].labelY + labelGap);
    }
    const overflow = (lines[lines.length - 1]?.labelY ?? maximumLabelY) - maximumLabelY;
    if (overflow > 0) {
      for (const line of lines) line.labelY -= overflow;
    }
    return lines;
  }

  function axisLabel(date: string) {
    const year = Number(date.slice(0, 4));
    const month = Number(date.slice(5, 7));
    const day = Number(date.slice(8, 10));
    if (kind === 'month') return `${day}日`;
    if (kind === 'recent' || kind === 'year' || (fixedRange && points.length <= 370)) {
      return `${month}/${day}`;
    }
    return `${year}/${month}`;
  }

  function indexFromPointer(event: PointerEvent | MouseEvent) {
    const bounds = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const ratio = Math.min(0.999999, Math.max(0, (event.clientX - bounds.left) / bounds.width));
    return Math.floor(ratio * points.length);
  }

  function selectFromPointer(event: PointerEvent) {
    if (!points.length || pinnedIndex !== null) return;
    hoverIndex = indexFromPointer(event);
  }

  function maximumScrollLeft() {
    return Math.max(0, viewportElement.scrollWidth - viewportElement.clientWidth);
  }

  function stopInertia() {
    if (inertiaFrame !== null) cancelAnimationFrame(inertiaFrame);
    inertiaFrame = null;
  }

  function resetPan() {
    if (panPointerId !== null && chartElement?.hasPointerCapture(panPointerId)) {
      chartElement.releasePointerCapture(panPointerId);
    }
    panPointerId = null;
    panning = false;
    panVelocity = 0;
    suppressClickUntil = 0;
  }

  function startInertia(initialVelocity: number) {
    stopInertia();
    let velocity = initialVelocity;
    let previousTime = performance.now();

    function move(timestamp: number) {
      const next = advanceChartInertia(
        viewportElement.scrollLeft,
        velocity,
        timestamp - previousTime,
        maximumScrollLeft(),
      );
      viewportElement.scrollLeft = next.scrollLeft;
      velocity = next.velocity;
      previousTime = timestamp;
      if (velocity === 0) {
        inertiaFrame = null;
        return;
      }
      inertiaFrame = requestAnimationFrame(move);
    }

    inertiaFrame = requestAnimationFrame(move);
  }

  function handlePointerDown(event: PointerEvent) {
    if (zoom <= 1.01 || !event.isPrimary || event.button !== 0) return;
    if (event.target instanceof Element && event.target.closest('.statistics-chart-tooltip'))
      return;

    stopInertia();
    panPointerId = event.pointerId;
    panStartX = event.clientX;
    panStartScrollLeft = viewportElement.scrollLeft;
    panLastScrollLeft = viewportElement.scrollLeft;
    panLastTime = event.timeStamp;
    panVelocity = 0;
    panning = false;
    suppressClickUntil = 0;
    chartElement.setPointerCapture(event.pointerId);
  }

  function handlePointerMove(event: PointerEvent) {
    if (panPointerId === event.pointerId) {
      const pointerDeltaX = event.clientX - panStartX;
      if (!panning && Math.abs(pointerDeltaX) >= panThreshold) {
        panning = true;
        hoverIndex = null;
      }
      if (panning) {
        const nextScrollLeft = chartPanScrollLeft(
          panStartScrollLeft,
          pointerDeltaX,
          maximumScrollLeft(),
        );
        const elapsedMs = Math.max(1, event.timeStamp - panLastTime);
        const frameVelocity = (nextScrollLeft - panLastScrollLeft) / elapsedMs;
        panVelocity = panVelocity * 0.65 + frameVelocity * 0.35;
        viewportElement.scrollLeft = nextScrollLeft;
        panLastScrollLeft = nextScrollLeft;
        panLastTime = event.timeStamp;
        event.preventDefault();
        return;
      }
    }
    selectFromPointer(event);
  }

  function handlePointerEnd(event: PointerEvent, cancelled = false) {
    if (panPointerId !== event.pointerId) return;
    if (chartElement.hasPointerCapture(event.pointerId)) {
      chartElement.releasePointerCapture(event.pointerId);
    }

    const wasPanning = panning;
    const idleMs = Math.max(0, event.timeStamp - panLastTime);
    const releaseVelocity = panVelocity * Math.max(0, 1 - idleMs / 100);
    panPointerId = null;
    panning = false;
    panVelocity = 0;

    if (!wasPanning) return;
    suppressClickUntil = event.timeStamp + 500;
    if (
      !cancelled &&
      Math.abs(releaseVelocity) >= 0.02 &&
      !window.matchMedia('(prefers-reduced-motion: reduce)').matches
    ) {
      startInertia(releaseVelocity);
    }
  }

  function handleWheel(event: WheelEvent) {
    if (
      pinnedIndex !== null &&
      event.target instanceof Node &&
      tooltipElement?.contains(event.target)
    ) {
      if (event.target instanceof Element) {
        const list = event.target.closest('.statistics-chart-tooltip ul');
        if (list && list.scrollHeight > list.clientHeight) return;
      }
      event.preventDefault();
      return;
    }
    stopInertia();
    const nextZoom = nextChartZoom(zoom, maximumZoom, event.deltaY, event.deltaMode);
    event.preventDefault();
    if (Math.abs(nextZoom - zoom) < 0.001) return;

    const bounds = viewportElement.getBoundingClientRect();
    const cursorX = event.clientX - bounds.left;
    const anchor = (viewportElement.scrollLeft + cursorX) / viewportElement.scrollWidth;
    zoom = nextZoom;
    requestAnimationFrame(() => {
      viewportElement.scrollLeft = anchor * viewportElement.scrollWidth - cursorX;
    });
  }

  function handleChartClick(event: MouseEvent) {
    if (event.timeStamp < suppressClickUntil) {
      suppressClickUntil = 0;
      return;
    }
    suppressClickUntil = 0;
    if (!points.length) return;
    const index = indexFromPointer(event);
    if (pinnedIndex !== null) {
      pinnedIndex = null;
      hoverIndex = index;
      return;
    }
    pinnedIndex = index;
    hoverIndex = null;
  }

  function handleKey(event: KeyboardEvent) {
    if (event.key === 'Escape' && pinnedIndex !== null) {
      event.preventDefault();
      hoverIndex = pinnedIndex;
      pinnedIndex = null;
      return;
    }
    if (!points.length) return;
    if (['Enter', ' '].includes(event.key)) {
      event.preventDefault();
      if (pinnedIndex === null) {
        pinnedIndex = activeIndex ?? points.length - 1;
        hoverIndex = null;
      }
      return;
    }
    if (pinnedIndex !== null || !['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key))
      return;
    event.preventDefault();
    if (event.key === 'Home') hoverIndex = 0;
    else if (event.key === 'End') hoverIndex = points.length - 1;
    else if (event.key === 'ArrowLeft')
      hoverIndex = Math.max(0, (activeIndex ?? points.length) - 1);
    else hoverIndex = Math.min(points.length - 1, (activeIndex ?? -1) + 1);
  }
</script>

<div bind:this={chartContainerElement} class="statistics-combined-chart">
  <div
    class="statistics-chart-y-axis daily"
    style={`height: ${chartPixelHeight}px`}
    aria-hidden="true"
  >
    {#each dailyAxisTicks as tick}<span
        class="statistics-chart-y-tick"
        style={`top: ${axisTickTop(tick.ratio)}px`}>{compactDuration(tick.value)}</span
      >{/each}<span class="statistics-chart-y-title" style={`top: ${axisTitleTop()}px`}>日別</span>
  </div>
  <div bind:this={viewportElement} class:zoomed={zoom > 1.01} class="statistics-chart-viewport">
    <div class="statistics-chart-stage" style={`width: ${zoom * 100}%`}>
      <div
        bind:this={chartElement}
        class="statistics-chart-interaction"
        role="slider"
        tabindex="0"
        aria-label="グラフの日付"
        aria-valuemin="0"
        aria-valuemax={Math.max(0, points.length - 1)}
        aria-valuenow={accessibleIndex}
        aria-valuetext={accessibleValue}
        class:draggable={zoom > 1.01}
        class:panning
        onpointerdown={handlePointerDown}
        onpointermove={handlePointerMove}
        onpointerup={handlePointerEnd}
        onpointercancel={(event) => handlePointerEnd(event, true)}
        onpointerleave={() => {
          if (pinnedIndex === null) hoverIndex = null;
        }}
        onfocus={() => {
          if (activeIndex === null && points.length) hoverIndex = points.length - 1;
        }}
        onkeydown={handleKey}
        onclick={handleChartClick}
      >
        <svg
          bind:this={chartSvgElement}
          class="statistics-chart"
          viewBox={`0 0 ${chartWidth} ${chartViewHeight}`}
          role="img"
          aria-label="日別プレイ時間と累計プレイ時間を重ねたグラフ"
        >
          <line
            class="statistics-chart-axis"
            x1="0"
            y1={plotBottom}
            x2={chartWidth}
            y2={plotBottom}
          />
          {#each dailyAxisTicks as tick}
            {#if tick.ratio > 0}
              <line
                class="statistics-chart-gridline"
                x1="0"
                y1={plotBottom - tick.ratio * plotHeight}
                x2={chartWidth}
                y2={plotBottom - tick.ratio * plotHeight}
              />
            {/if}
          {/each}
          {#each stackedBars as bar}
            <line
              class="statistics-chart-bar-segment"
              x1={bar.x}
              y1={bar.bottom}
              x2={bar.x}
              y2={bar.top}
              style={`stroke: ${bar.color}; stroke-width: ${barStrokeWidth}`}
            />
          {/each}
          {#if points.length}
            <polyline class="statistics-chart-line" points={cumulativePoints} />
            {#if points.length === 1}
              <circle
                class="statistics-chart-single-point"
                cx={pointX(0, 1, chartWidth)}
                cy={plotBottom - (points[0].cumulative_seconds / maximumCumulative) * plotHeight}
                r="3.5"
              />
            {/if}
          {/if}
          {#each points as point, index}
            {#if index % labelStep === 0 || index === points.length - 1}
              <text
                class="statistics-chart-label"
                x={pointX(index, points.length, chartWidth)}
                y={chartViewHeight - 14}
                text-anchor="middle">{axisLabel(point.date)}</text
              >
            {/if}
          {/each}
          {#each timestampLines as marker}
            <g class="statistics-chart-timestamp">
              <title>{marker.label}</title>
              <line x1="0" y1={marker.y} x2={chartWidth} y2={marker.y} />
              <text x={chartWidth - 6} y={marker.labelY} text-anchor="end">{marker.label}</text>
            </g>
          {/each}
          {#if activePoint && activeIndex !== null}
            {@const x = pointX(activeIndex, points.length, chartWidth)}
            {@const dailyY =
              plotBottom - (activePoint.playtime_seconds / maximumDaily) * plotHeight}
            {@const cumulativeY =
              plotBottom - (activePoint.cumulative_seconds / maximumCumulative) * plotHeight}
            <line class="statistics-chart-cursor" x1={x} y1={plotTop} x2={x} y2={plotBottom} />
            <circle class="statistics-chart-daily-point" cx={x} cy={dailyY} r="4" />
            <circle class="statistics-chart-cumulative-point" cx={x} cy={cumulativeY} r="4" />
          {/if}
        </svg>
      </div>
    </div>
  </div>
  {#if activePoint}
    <div
      bind:this={tooltipElement}
      class:pinned={pinnedIndex !== null}
      class:align-start={tooltipAlignStart}
      class:align-end={tooltipAlignEnd}
      class="statistics-chart-tooltip"
      style={`left: ${tooltipLeft}px`}
      aria-live="polite"
    >
      <strong>{formatDateKey(activePoint.date)}</strong>
      <dl>
        <div>
          <dt>合計</dt>
          <dd>{compactDuration(activePoint.playtime_seconds)}</dd>
        </div>
        <div>
          <dt>累計</dt>
          <dd>{compactDuration(activePoint.cumulative_seconds)}</dd>
        </div>
      </dl>
      {#if showGameBreakdown}
        {#if activeGames.length}
          <ul>
            {#each activeGames as game}<li style={`--game-color: ${gameColor(game.game_id)}`}>
                <span class="statistics-chart-game">
                  <span class="statistics-chart-game-image">
                    {#if game.thumbnail_path}<img
                        src={imageSrc(game.thumbnail_path)}
                        alt=""
                      />{:else}<i></i>{/if}
                  </span>
                  <span class="statistics-chart-game-title">{game.title}</span>
                </span>
                <strong>{compactDuration(game.playtime_seconds)}</strong>
              </li>{/each}
          </ul>
        {:else}
          <p>プレイ記録なし</p>
        {/if}
      {/if}
    </div>
  {/if}
  {#if zoom > 1.01}
    <span class="statistics-chart-edge-fade start" aria-hidden="true"></span>
    <span class="statistics-chart-edge-fade end" aria-hidden="true"></span>
    <span class="statistics-chart-zoom" aria-live="polite">{Math.round(zoom * 100)}%</span>
  {/if}
  <div
    class="statistics-chart-y-axis cumulative"
    style={`height: ${chartPixelHeight}px`}
    aria-hidden="true"
  >
    {#each cumulativeAxisTicks as tick}<span
        class="statistics-chart-y-tick"
        style={`top: ${axisTickTop(tick.ratio)}px`}>{compactDuration(tick.value)}</span
      >{/each}<span class="statistics-chart-y-title" style={`top: ${axisTitleTop()}px`}>累計</span>
  </div>
</div>

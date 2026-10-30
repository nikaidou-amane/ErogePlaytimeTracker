import type { GameTimestamp, StatisticsDay } from './types';

export interface DailyTrendPoint extends StatisticsDay {
  cumulative_seconds: number;
}

export interface ChartAxisTick {
  ratio: number;
  value: number;
}

export interface ChartInertiaStep {
  scrollLeft: number;
  velocity: number;
}

export interface TimestampMarkerGroup {
  playtimeSeconds: number;
  names: string[];
}

export function compactDuration(seconds: number) {
  const totalMinutes = Math.floor(Math.max(0, seconds) / 60);
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  if (hours) return minutes ? `${hours}時間 ${minutes}分` : `${hours}時間`;
  if (totalMinutes) return `${totalMinutes}分`;
  return seconds > 0 ? `${Math.floor(seconds)}秒` : '0分';
}

export function formatDateKey(value: string) {
  const [year, month, day] = value.split('-').map(Number);
  return `${year}年${month}月${day}日`;
}

export function dailyTrend(days: StatisticsDay[]): DailyTrendPoint[] {
  let cumulative = 0;
  return days.map((day) => {
    cumulative += day.playtime_seconds;
    return { ...day, cumulative_seconds: cumulative };
  });
}

export function timestampMarkerGroups(
  timestamps: Pick<GameTimestamp, 'name' | 'playtime_seconds'>[],
): TimestampMarkerGroup[] {
  const groups = new Map<number, string[]>();
  for (const point of timestamps) {
    const playtimeSeconds = Math.max(0, point.playtime_seconds);
    const names = groups.get(playtimeSeconds) ?? [];
    names.push(point.name);
    groups.set(playtimeSeconds, names);
  }
  return Array.from(groups, ([playtimeSeconds, names]) => ({ playtimeSeconds, names })).sort(
    (left, right) => right.playtimeSeconds - left.playtimeSeconds,
  );
}

export function chartAxisTicks(maximumSeconds: number): ChartAxisTick[] {
  const maximum = Math.max(60, Math.round(maximumSeconds));
  return [0, 1 / 3, 2 / 3, 1].map((ratio) => ({
    ratio,
    value: Math.round(maximum * ratio),
  }));
}

export function nextChartZoom(current: number, maximum: number, deltaY: number, deltaMode: number) {
  const multiplier = deltaMode === 1 ? 16 : deltaMode === 2 ? 100 : 1;
  const delta = deltaY * multiplier;
  if (!delta) return current;
  const exponent =
    Math.abs(delta) < 20 ? Math.abs(delta) * 0.015 : Math.max(0.3, Math.abs(delta) * 0.0015);
  const factor = Math.exp(-Math.sign(delta) * exponent);
  return Math.min(maximum, Math.max(1, current * factor));
}

export function chartPanScrollLeft(
  startScrollLeft: number,
  pointerDeltaX: number,
  maximumScrollLeft: number,
) {
  return Math.min(maximumScrollLeft, Math.max(0, startScrollLeft - pointerDeltaX));
}

export interface ChartScale {
  scale: number;
  offset: number;
}

/**
 * Resolves how a viewBox maps onto the rendered SVG box. The y-axis labels are drawn
 * outside the SVG, so they have to repeat this mapping to line up with the gridlines.
 * Both values come from the live client size, which keeps them correct after any relayout.
 */
export function chartScale(
  viewBoxWidth: number,
  viewBoxHeight: number,
  clientWidth: number,
  clientHeight: number,
): ChartScale {
  if (!clientWidth || !clientHeight || !viewBoxWidth || !viewBoxHeight) {
    return { scale: 1, offset: 0 };
  }
  const scale = Math.min(clientWidth / viewBoxWidth, clientHeight / viewBoxHeight);
  return { scale, offset: (clientHeight - viewBoxHeight * scale) / 2 };
}

/** Converts a viewBox y coordinate into the pixel offset the axis label is placed at. */
export function chartAxisTop({ scale, offset }: ChartScale, viewBoxY: number) {
  return offset + viewBoxY * scale;
}

export function advanceChartInertia(
  scrollLeft: number,
  velocity: number,
  elapsedMs: number,
  maximumScrollLeft: number,
): ChartInertiaStep {
  const frameDuration = Math.min(32, Math.max(0, elapsedMs));
  const speed = Math.abs(velocity);
  const nextSpeed = Math.max(0, speed - 0.0035 * frameDuration);
  const distance = Math.sign(velocity) * ((speed + nextSpeed) / 2) * frameDuration;
  const nextScrollLeft = Math.min(maximumScrollLeft, Math.max(0, scrollLeft + distance));

  if (nextSpeed === 0 || nextScrollLeft === 0 || nextScrollLeft === maximumScrollLeft) {
    return { scrollLeft: nextScrollLeft, velocity: 0 };
  }
  return { scrollLeft: nextScrollLeft, velocity: Math.sign(velocity) * nextSpeed };
}

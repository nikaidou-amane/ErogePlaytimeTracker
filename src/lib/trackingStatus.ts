import type { TrackingStatus } from './types';

export type TrackingGame = TrackingStatus['games'][number];
export type TrackingPhase = TrackingGame['phase'];
export interface TrackingStatusGroup {
  phase: TrackingPhase;
  games: TrackingGame[];
}

const phaseOrder: TrackingPhase[] = ['foreground', 'background', 'starting', 'window_transition'];

/** Short phase names used by both the header chips and their tooltip. */
export const phaseLabels: Record<TrackingPhase, string> = {
  foreground: 'プレイ中',
  background: 'バックグラウンド',
  starting: '起動中',
  window_transition: '画面切替・終了中',
};

export function trackingStatusGroups(status: TrackingStatus): TrackingStatusGroup[] {
  return phaseOrder
    .map((phase) => ({ phase, games: status.games.filter((game) => game.phase === phase) }))
    .filter((group) => group.games.length > 0);
}

export interface TrackingStatusChip {
  phase: TrackingPhase;
  /** Visible chip text without the leading bullet, e.g. `プレイ中 · Game`. */
  label: string;
}

export function trackingStatusChips(groups: TrackingStatusGroup[]): TrackingStatusChip[] {
  return groups.map((group) => ({
    phase: group.phase,
    label: `${phaseLabels[group.phase]} · ${
      group.games.length === 1 ? group.games[0].title : `${group.games.length}本`
    }`,
  }));
}

/**
 * Distributes the header chips over two rows. The highest priority phase keeps
 * the first row and the remaining ones (background, starting, transition) move
 * to the second row, so a long background label no longer competes with the
 * foreground game for a single line.
 */
export function trackingStatusRows<T>(items: T[]): T[][] {
  if (items.length === 0) return [];
  if (items.length === 1) return [items];
  return [items.slice(0, 1), items.slice(1)];
}

/**
 * The text the header actually shows, one string per rendered line. The tooltip
 * reuses this so it can never drift from the visible labels.
 */
export function trackingStatusLines(rows: TrackingStatusChip[][]): string[] {
  return rows.map((row) => row.map((chip) => `● ${chip.label}`).join(' '));
}

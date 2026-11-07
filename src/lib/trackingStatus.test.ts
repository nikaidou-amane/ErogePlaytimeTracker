import { describe, expect, it } from 'vitest';
import {
  trackingStatusChips,
  trackingStatusGroups,
  trackingStatusLines,
  trackingStatusRows,
} from './trackingStatus';
import type { TrackingStatus } from './types';

const game = (game_id: number, title: string, phase: TrackingStatus['games'][number]['phase']) => ({
  game_id,
  title,
  phase,
  session_id: phase === 'starting' ? null : game_id + 100,
  session_seconds: phase === 'starting' ? null : 90,
});

describe('tracking status presentation', () => {
  it('labels a single starting game with its title', () => {
    const groups = trackingStatusGroups({
      is_paused: false,
      pause_started_at: null,
      games: [game(1, 'Game A', 'starting')],
    });
    expect(groups[0].games[0].session_id).toBeNull();
    expect(trackingStatusChips(groups).map((chip) => chip.label)).toEqual(['起動中 · Game A']);
  });

  it('shows foreground, background and starting states together', () => {
    const groups = trackingStatusGroups({
      is_paused: false,
      pause_started_at: null,
      games: [
        game(1, 'Game A', 'background'),
        game(2, 'Game B', 'foreground'),
        game(3, 'Game C', 'starting'),
      ],
    });
    expect(groups.map((group) => group.phase)).toEqual(['foreground', 'background', 'starting']);
    expect(trackingStatusChips(groups).map((chip) => chip.label)).toEqual([
      'プレイ中 · Game B',
      'バックグラウンド · Game A',
      '起動中 · Game C',
    ]);
  });

  it('groups multiple games in the same phase by count', () => {
    const groups = trackingStatusGroups({
      is_paused: false,
      pause_started_at: null,
      games: [game(1, 'Game A', 'background'), game(2, 'Game B', 'background')],
    });
    expect(trackingStatusChips(groups).map((chip) => chip.label)).toEqual([
      'バックグラウンド · 2本',
    ]);
  });

  it('moves the lower priority phases onto a second row', () => {
    const groups = trackingStatusGroups({
      is_paused: false,
      pause_started_at: null,
      games: [
        game(1, 'Game A', 'background'),
        game(2, 'Game B', 'foreground'),
        game(3, 'Game C', 'starting'),
      ],
    });
    // The foreground game keeps the first row, so the background label is not
    // squeezed against it on a narrow window.
    expect(trackingStatusRows(groups).map((row) => row.map((entry) => entry.phase))).toEqual([
      ['foreground'],
      ['background', 'starting'],
    ]);
    expect(trackingStatusRows(groups)[1].map((entry) => entry.games.length)).toEqual([1, 1]);
  });

  it('keeps a single state on one row and preserves its order', () => {
    const one = trackingStatusGroups({
      is_paused: false,
      pause_started_at: null,
      games: [game(1, 'Game A', 'foreground')],
    });
    expect(trackingStatusRows(one).map((row) => row.map((entry) => entry.phase))).toEqual([
      ['foreground'],
    ]);
    expect(trackingStatusRows([])).toEqual([]);
    expect(trackingStatusRows([1, 2, 3])).toEqual([[1], [2, 3]]);
  });

  it('builds the tooltip from the same labels the header renders', () => {
    const groups = trackingStatusGroups({
      is_paused: false,
      pause_started_at: null,
      games: [
        game(1, 'Game A', 'background'),
        game(2, 'Game B', 'foreground'),
        game(3, 'Game C', 'starting'),
      ],
    });
    const rows = trackingStatusRows(trackingStatusChips(groups));
    const lines = trackingStatusLines(rows);
    // One line per rendered row, each prefixed like the visible text.
    expect(lines).toEqual(['● プレイ中 · Game B', '● バックグラウンド · Game A ● 起動中 · Game C']);
    // Every visible label appears verbatim in the tooltip.
    for (const chip of trackingStatusChips(groups))
      expect(lines.join('\n')).toContain(`● ${chip.label}`);
  });
});

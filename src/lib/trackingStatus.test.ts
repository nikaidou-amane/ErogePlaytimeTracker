import { describe, expect, it } from 'vitest';
import {
  currentSessionSeconds,
  sessionMinutes,
  trackingStatusChips,
  trackingStatusGroups,
  trackingStatusLines,
  trackingStatusRows,
} from './trackingStatus';
import type { TrackingStatus } from './types';

const game = (
  game_id: number,
  title: string,
  phase: TrackingStatus['games'][number]['phase'],
  session_seconds: number | null = phase === 'starting' ? null : 90,
) => ({
  game_id,
  title,
  phase,
  session_id: phase === 'starting' ? null : game_id + 100,
  session_seconds,
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
    const sessionLine = '● このセッション · 1時間 2分';
    const lines = trackingStatusLines(rows, sessionLine);
    // One line per rendered row plus the session line, each prefixed like the
    // visible text.
    expect(lines).toEqual([
      '● プレイ中 · Game B',
      '● バックグラウンド · Game A ● 起動中 · Game C',
      '● このセッション · 1時間 2分',
    ]);
    // Every visible label appears verbatim in the tooltip.
    for (const chip of trackingStatusChips(groups))
      expect(lines.join('\n')).toContain(`● ${chip.label}`);
    // Without a session line the tooltip stays on the chips only.
    expect(trackingStatusLines(rows)).toEqual([
      '● プレイ中 · Game B',
      '● バックグラウンド · Game A ● 起動中 · Game C',
    ]);
  });

  it('formats the session duration in whole minutes, truncated', () => {
    // Seconds are dropped rather than rounded, so the header never runs ahead.
    expect(sessionMinutes(0)).toBe('0分');
    expect(sessionMinutes(59)).toBe('0分');
    expect(sessionMinutes(60)).toBe('1分');
    expect(sessionMinutes(119)).toBe('1分');
    expect(sessionMinutes(3599)).toBe('59分');
    expect(sessionMinutes(3600)).toBe('1時間 0分');
    expect(sessionMinutes(3725)).toBe('1時間 2分');
    // Past the hour the minutes part still truncates.
    expect(sessionMinutes(7325)).toBe('2時間 2分');
  });

  it('only reports a session duration when exactly one game is running', () => {
    expect(
      currentSessionSeconds({ is_paused: false, pause_started_at: null, games: [] }),
    ).toBeNull();
    expect(
      currentSessionSeconds({
        is_paused: false,
        pause_started_at: null,
        games: [game(1, 'Game A', 'starting')],
      }),
    ).toBeNull();
    expect(
      currentSessionSeconds({
        is_paused: false,
        pause_started_at: null,
        games: [game(1, 'Game A', 'foreground', 30)],
      }),
    ).toBe(30);
    // Each game has its own session length, so several games show no duration.
    expect(
      currentSessionSeconds({
        is_paused: false,
        pause_started_at: null,
        games: [game(1, 'Game A', 'foreground', 30), game(2, 'Game B', 'background', 45)],
      }),
    ).toBeNull();
  });
});

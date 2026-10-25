import { describe, expect, it } from 'vitest';
import { trackingStatusGroups, trackingStatusText } from './trackingStatus';
import type { TrackingStatus } from './types';

const game = (game_id: number, title: string, phase: TrackingStatus['games'][number]['phase']) => ({
  game_id,
  title,
  phase,
  session_id: phase === 'starting' ? null : game_id + 100,
});

describe('tracking status presentation', () => {
  it('shows a single starting game by title', () => {
    const groups = trackingStatusGroups({ games: [game(1, 'Game A', 'starting')], paused: false });
    expect(groups[0].games[0].session_id).toBeNull();
    expect(trackingStatusText(groups[0])).toBe('Game Aを起動中');
  });

  it('shows foreground, background and starting states together', () => {
    const groups = trackingStatusGroups({
      games: [
        game(1, 'Game A', 'background'),
        game(2, 'Game B', 'foreground'),
        game(3, 'Game C', 'starting'),
      ],
      paused: false,
    });
    expect(groups.map((group) => group.phase)).toEqual(['foreground', 'background', 'starting']);
    expect(groups.map(trackingStatusText)).toEqual([
      'Game Bをプレイ中',
      'Game A：バックグラウンド',
      'Game Cを起動中',
    ]);
  });

  it('groups multiple games in the same phase by count', () => {
    const groups = trackingStatusGroups({
      games: [game(1, 'Game A', 'background'), game(2, 'Game B', 'background')],
      paused: false,
    });
    expect(trackingStatusText(groups[0])).toBe('バックグラウンド 2本');
  });
});

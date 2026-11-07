import { describe, expect, it } from 'vitest';
import {
  defaultLibraryView,
  filterSummary,
  normalizeLibraryView,
  toggleValue,
} from './libraryFilters';

describe('library view preferences', () => {
  it('uses the defaults when nothing has been saved', () => {
    expect(normalizeLibraryView(null)).toEqual(defaultLibraryView);
    expect(normalizeLibraryView(undefined)).toEqual(defaultLibraryView);
  });

  it('keeps multiple brands, statuses, sort and view mode', () => {
    const saved = {
      brands: ['ブランドA', 'ブランドB'],
      playStatuses: ['playing', 'completed'],
      sort: 'total_playtime',
      descending: false,
      viewMode: 'table',
    };
    expect(normalizeLibraryView(saved)).toEqual(saved);
  });

  it('drops unknown statuses, sorts and view modes', () => {
    const saved = {
      brands: ['A'],
      playStatuses: ['playing', 'unknown'],
      sort: 'unknown',
      descending: true,
      viewMode: 'carousel',
    };
    expect(normalizeLibraryView(saved)).toEqual({
      ...defaultLibraryView,
      brands: ['A'],
      playStatuses: ['playing'],
    });
  });

  it('removes blank and duplicate brands', () => {
    const saved = {
      brands: ['A', '  ', 'A', 'B'],
      playStatuses: ['playing', 'playing'],
      sort: 'title',
      descending: false,
      viewMode: 'list',
    };
    expect(normalizeLibraryView(saved)).toEqual({
      brands: ['A', 'B'],
      playStatuses: ['playing'],
      sort: 'title',
      descending: false,
      viewMode: 'list',
    });
  });

  it.each([null, '[]', 'true', '42', '"text"', []])(
    'falls back to defaults for malformed saved data: %s',
    (saved) => {
      expect(normalizeLibraryView(saved)).toEqual(defaultLibraryView);
    },
  );

  it('adds and removes a selection, where an empty list means all', () => {
    expect(toggleValue<string>([], 'A')).toEqual(['A']);
    expect(toggleValue(['A'], 'B')).toEqual(['A', 'B']);
    expect(toggleValue(['A', 'B'], 'A')).toEqual(['B']);
  });

  it('shows the single selected label instead of a count', () => {
    expect(filterSummary([], 'すべてのブランド')).toBe('すべてのブランド');
    expect(filterSummary(['ブランドA'], 'すべてのブランド')).toBe('ブランドA');
    expect(filterSummary(['ブランドA', 'ブランドB'], 'すべてのブランド')).toBe('2件選択');
    expect(filterSummary(['攻略中', '未プレイ', '完了'], 'すべての状況')).toBe('3件選択');
  });
});

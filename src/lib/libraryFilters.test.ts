import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { loadLibraryFilters, saveLibraryFilters, type LibraryFilters } from './libraryFilters';

describe('library filter preferences', () => {
  let values: Map<string, string>;
  const defaults: LibraryFilters = {
    search: '',
    brand: '',
    playStatus: '',
    sort: 'last_played',
    descending: true,
  };

  beforeEach(() => {
    values = new Map();
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
    });
  });

  afterEach(() => vi.unstubAllGlobals());

  it('uses the existing defaults when no filters have been saved', () => {
    expect(loadLibraryFilters()).toEqual(defaults);
  });

  it('restores all filters and ascending sort on subsequent loads', () => {
    const filters: LibraryFilters = {
      search: 'タイトル | 続編',
      brand: 'ブランド名',
      playStatus: 'playing',
      sort: 'total_playtime',
      descending: false,
    };
    saveLibraryFilters(filters);
    expect(loadLibraryFilters()).toEqual(filters);
    expect(loadLibraryFilters()).toEqual(filters);
  });

  it('keeps cleared filters and the latest sort direction', () => {
    saveLibraryFilters({ ...defaults, search: '検索', brand: 'ブランド', playStatus: 'completed' });
    saveLibraryFilters({ ...defaults, sort: 'title', descending: false });
    expect(loadLibraryFilters()).toEqual({ ...defaults, sort: 'title', descending: false });
  });

  it('defaults missing or invalid fields while preserving valid ones', () => {
    values.set(
      'library-filters',
      JSON.stringify({
        search: '検索',
        brand: 42,
        playStatus: 'unknown',
        sort: 'unknown',
        descending: 'false',
      }),
    );
    expect(loadLibraryFilters()).toEqual({ ...defaults, search: '検索' });
    values.set('library-filters', JSON.stringify({ sort: 'session_count' }));
    expect(loadLibraryFilters()).toEqual({ ...defaults, sort: 'session_count' });
  });

  it.each(['invalid JSON', 'null', '[]', 'true', '42', '"text"'])(
    'uses defaults for malformed saved data: %s',
    (saved) => {
      values.set('library-filters', saved);
      expect(loadLibraryFilters()).toEqual(defaults);
    },
  );

  it('allows filtering even when storage access fails', () => {
    vi.stubGlobal('localStorage', {
      getItem: () => {
        throw new Error('Storage unavailable');
      },
      setItem: () => {
        throw new Error('Storage full');
      },
    });
    expect(loadLibraryFilters()).toEqual(defaults);
    expect(() => saveLibraryFilters(defaults)).not.toThrow();
  });
});

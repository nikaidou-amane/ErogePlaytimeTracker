import type { PlayStatus, SortKey } from './types';

export interface LibraryFilters {
  search: string;
  brand: string;
  playStatus: PlayStatus | '';
  sort: SortKey;
  descending: boolean;
}

const storageKey = 'library-filters';
const defaults: LibraryFilters = {
  search: '',
  brand: '',
  playStatus: '',
  sort: 'last_played',
  descending: true,
};
const sortKeys: SortKey[] = [
  'title',
  'brand',
  'release_date',
  'created_at',
  'total_playtime',
  'last_played',
  'session_count',
];
const playStatuses: PlayStatus[] = ['unplayed', 'playing', 'completed', 'retired'];

export function loadLibraryFilters(): LibraryFilters {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(storageKey) ?? '{}');
    if (!value || typeof value !== 'object' || Array.isArray(value)) return { ...defaults };
    const saved = value as Record<string, unknown>;
    return {
      search: typeof saved.search === 'string' ? saved.search : defaults.search,
      brand: typeof saved.brand === 'string' ? saved.brand : defaults.brand,
      playStatus: playStatuses.find((status) => status === saved.playStatus) ?? defaults.playStatus,
      sort: sortKeys.find((key) => key === saved.sort) ?? defaults.sort,
      descending: typeof saved.descending === 'boolean' ? saved.descending : defaults.descending,
    };
  } catch {
    return { ...defaults };
  }
}

export function saveLibraryFilters(filters: LibraryFilters) {
  try {
    localStorage.setItem(storageKey, JSON.stringify(filters));
  } catch {
    // Filtering remains usable when browser storage is unavailable.
  }
}

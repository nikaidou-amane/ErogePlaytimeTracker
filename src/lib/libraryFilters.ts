import type { LibraryView, LibraryViewMode, PlayStatus, SortKey } from './types';

export const sortKeys: SortKey[] = [
  'title',
  'brand',
  'release_date',
  'created_at',
  'total_playtime',
  'last_played',
  'session_count',
];
export const playStatuses: PlayStatus[] = ['unplayed', 'playing', 'completed', 'retired'];
export const viewModes: LibraryViewMode[] = ['grid', 'list', 'table'];

export const defaultLibraryView: LibraryView = {
  brands: [],
  playStatuses: [],
  sort: 'last_played',
  descending: true,
  viewMode: 'grid',
};

/** 未知の値や型違いが混ざっていても画面が開けるよう既定値へ寄せる。 */
export function normalizeLibraryView(value: unknown): LibraryView {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return { ...defaultLibraryView };
  }
  const saved = value as Record<string, unknown>;
  const brands = Array.isArray(saved.brands)
    ? saved.brands.filter(
        (brand): brand is string => typeof brand === 'string' && brand.trim() !== '',
      )
    : [];
  const statuses = Array.isArray(saved.playStatuses)
    ? playStatuses.filter((status) => (saved.playStatuses as unknown[]).includes(status))
    : [];
  return {
    brands: Array.from(new Set(brands)),
    playStatuses: Array.from(new Set(statuses)),
    sort: sortKeys.find((key) => key === saved.sort) ?? defaultLibraryView.sort,
    descending:
      typeof saved.descending === 'boolean' ? saved.descending : defaultLibraryView.descending,
    viewMode: viewModes.find((mode) => mode === saved.viewMode) ?? defaultLibraryView.viewMode,
  };
}

/** 選択の追加・解除。空配列は「すべて」を意味する。 */
export function toggleValue<T>(values: T[], value: T): T[] {
  return values.includes(value) ? values.filter((item) => item !== value) : [...values, value];
}

/** 選択が1件ならその名前、複数なら件数、未選択なら「すべて」を出す。 */
export function filterSummary(labels: string[], allLabel: string) {
  if (!labels.length) return allLabel;
  if (labels.length === 1) return labels[0];
  return `${labels.length}件選択`;
}

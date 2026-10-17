import type { MetadataProvider } from './types';

export function metadataSourceLabel(provider: MetadataProvider | null | undefined) {
  return provider === 'vndb'
    ? 'VNDB'
    : provider === 'erogamescape'
      ? 'ErogameScape'
      : '取得元サイト';
}

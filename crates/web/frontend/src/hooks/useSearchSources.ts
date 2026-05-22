import { useQuery } from '@tanstack/react-query';
import { rpc } from '../lib/rpc';
import { SourceSearchResult } from '../lib/types';

/** Minimum query length before we hit the backend, to avoid noise. */
export const MIN_SEARCH_LEN = 2;

/**
 * Full-text search across every contract's source via `edb_searchSources`.
 * Disabled until the (already-debounced) query reaches {@link MIN_SEARCH_LEN}.
 */
export function useSearchSources(query: string) {
  const trimmed = query.trim();
  return useQuery({
    queryKey: ['search-sources', trimmed] as const,
    queryFn: ({ signal }) => rpc('edb_searchSources', SourceSearchResult, [trimmed], { signal }),
    enabled: trimmed.length >= MIN_SEARCH_LEN,
    staleTime: 30_000,
  });
}

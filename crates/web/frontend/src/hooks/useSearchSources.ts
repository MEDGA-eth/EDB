import { useQuery } from '@tanstack/react-query';
import { rpc } from '../lib/rpc';
import { SourceSearchResult } from '../lib/types';

/** Minimum query length before we hit the backend, to avoid noise. */
export const MIN_SEARCH_LEN = 2;

/**
 * Full-text search across every contract's source via `edb_searchSources`.
 * Disabled until the (already-debounced) query reaches {@link MIN_SEARCH_LEN}.
 * When `regex` is true the query is sent as a case-insensitive regular
 * expression; otherwise as a case-insensitive substring.
 */
export function useSearchSources(query: string, regex = false) {
  const trimmed = query.trim();
  return useQuery({
    queryKey: ['search-sources', trimmed, regex] as const,
    queryFn: ({ signal }) =>
      rpc('edb_searchSources', SourceSearchResult, [trimmed, regex], { signal }),
    enabled: trimmed.length >= MIN_SEARCH_LEN,
    staleTime: 30_000,
  });
}

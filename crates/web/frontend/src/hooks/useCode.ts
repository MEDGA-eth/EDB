import { useQuery } from '@tanstack/react-query';
import { rpc } from '../lib/rpc';
import { CodeKind } from '../lib/types';

export function useCode(id: number) {
  return useQuery({
    queryKey: ['code', id] as const,
    queryFn: () => rpc('edb_getCode', CodeKind, [id]),
    enabled: id >= 0,
  });
}

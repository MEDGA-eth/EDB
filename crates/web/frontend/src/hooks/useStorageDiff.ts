import { useQuery } from '@tanstack/react-query';
import { rpc } from '../lib/rpc';
import { StorageDiff } from '../lib/types';

export function useStorageDiff(id: number) {
  return useQuery({
    queryKey: ['storage-diff', id] as const,
    queryFn: () => rpc('edb_getStorageDiff', StorageDiff, [id]),
    enabled: id >= 0,
  });
}

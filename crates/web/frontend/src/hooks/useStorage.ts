import { useQuery } from '@tanstack/react-query';
import { rpc } from '../lib/rpc';
import { U256 } from '../lib/types';

export function useStorage(id: number, slot: string | undefined) {
  return useQuery({
    queryKey: ['storage', id, slot] as const,
    queryFn: () => rpc('edb_getStorage', U256, [id, slot]),
    enabled: id >= 0 && !!slot,
  });
}

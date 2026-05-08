import { useQuery } from '@tanstack/react-query';
import { rpc } from '../lib/rpc';
import { SnapshotInfo } from '../lib/types';

export function useSnapshotInfo(id: number) {
  return useQuery({
    queryKey: ['snapshot', id] as const,
    queryFn: () => rpc('edb_getSnapshotInfo', SnapshotInfo, [id]),
    enabled: id >= 0,
  });
}

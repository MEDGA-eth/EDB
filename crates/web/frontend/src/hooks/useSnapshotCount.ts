import { useQuery } from '@tanstack/react-query';
import { z } from 'zod';
import { rpc } from '../lib/rpc';

const Schema = z.number().int().nonnegative();

export function useSnapshotCount() {
  return useQuery({
    queryKey: ['count'] as const,
    queryFn: () => rpc('edb_getSnapshotCount', Schema),
  });
}

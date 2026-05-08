import { useQuery } from '@tanstack/react-query';
import { z } from 'zod';
import { rpc } from '../lib/rpc';

export function useNextCall(id: number) {
  return useQuery({
    queryKey: ['next-call', id] as const,
    queryFn: () => rpc('edb_getNextCall', z.number().int().nonnegative(), [id]),
    enabled: id >= 0,
  });
}

import { useQuery } from '@tanstack/react-query';
import { z } from 'zod';
import { rpc } from '../lib/rpc';

export function usePrevCall(id: number) {
  return useQuery({
    queryKey: ['prev-call', id] as const,
    queryFn: () => rpc('edb_getPrevCall', z.number().int().nonnegative(), [id]),
    enabled: id >= 0,
  });
}

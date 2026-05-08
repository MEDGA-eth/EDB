import { useQuery } from '@tanstack/react-query';
import { rpc } from '../lib/rpc';
import { Trace } from '../lib/types';

export function useTrace() {
  return useQuery({
    queryKey: ['trace'] as const,
    queryFn: () => rpc('edb_getTrace', Trace),
  });
}

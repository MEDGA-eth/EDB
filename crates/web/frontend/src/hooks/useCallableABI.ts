import { useQuery } from '@tanstack/react-query';
import { rpc } from '../lib/rpc';
import { CallableAbi } from '../lib/types';

export function useCallableABI(addr: string | undefined) {
  return useQuery({
    queryKey: ['callable', addr] as const,
    queryFn: () => rpc('edb_getCallableABI', CallableAbi, [addr]),
    enabled: !!addr,
  });
}

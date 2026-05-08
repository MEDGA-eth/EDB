import { useQuery } from '@tanstack/react-query';
import { rpc } from '../lib/rpc';
import { Abi } from '../lib/types';

export function useContractABI(addr: string | undefined) {
  return useQuery({
    queryKey: ['abi', addr] as const,
    queryFn: () => rpc('edb_getContractABI', Abi, [addr]),
    enabled: !!addr,
  });
}

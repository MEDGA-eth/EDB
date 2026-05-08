import { useQuery } from '@tanstack/react-query';
import { rpc } from '../lib/rpc';
import { CodeKind } from '../lib/types';

export function useCodeByAddress(addr: string | undefined) {
  return useQuery({
    queryKey: ['code-addr', addr] as const,
    queryFn: () => rpc('edb_getCodeByAddress', CodeKind, [addr]),
    enabled: !!addr,
  });
}

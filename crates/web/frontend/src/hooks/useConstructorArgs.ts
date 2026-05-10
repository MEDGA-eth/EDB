import { useQuery } from '@tanstack/react-query';
import { rpc } from '../lib/rpc';
import { ConstructorArgs } from '../lib/types';

/**
 * `edb_getConstructorArgs` returns `Option<Bytes>`, a single hex string
 * holding the ABI-encoded constructor argument blob, or `null` if the
 * artifact didn't capture deploy-time arguments.
 */
export function useConstructorArgs(addr: string | undefined) {
  return useQuery({
    queryKey: ['ctor', addr] as const,
    queryFn: () => rpc('edb_getConstructorArgs', ConstructorArgs, [addr]),
    enabled: !!addr,
  });
}

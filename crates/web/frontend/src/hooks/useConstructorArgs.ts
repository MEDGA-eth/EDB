import { useQuery } from '@tanstack/react-query';
import { z } from 'zod';
import { rpc } from '../lib/rpc';

const Schema = z.array(z.unknown());

export function useConstructorArgs(addr: string | undefined) {
  return useQuery({
    queryKey: ['ctor', addr] as const,
    queryFn: () => rpc('edb_getConstructorArgs', Schema, [addr]),
    enabled: !!addr,
  });
}

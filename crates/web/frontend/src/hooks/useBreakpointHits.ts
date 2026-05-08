import { useQuery } from '@tanstack/react-query';
import { z } from 'zod';
import { rpc } from '../lib/rpc';
import type { Breakpoint } from '../lib/types';

const Schema = z.array(z.number().int().nonnegative());

function stableHash(bp: Breakpoint): string {
  return JSON.stringify(bp, Object.keys(bp).sort());
}

export function useBreakpointHits(bp: Breakpoint | null) {
  return useQuery({
    queryKey: ['bp-hits', bp ? stableHash(bp) : null] as const,
    queryFn: () => rpc('edb_getBreakpointHits', Schema, [bp]),
    enabled: bp !== null,
  });
}

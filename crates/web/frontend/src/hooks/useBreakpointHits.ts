import { useQuery } from '@tanstack/react-query';
import { z } from 'zod';
import { rpc } from '../lib/rpc';
import { breakpointToWire, type Breakpoint } from '../lib/types';

const Schema = z.array(z.number().int().nonnegative());

function stableHash(bp: Breakpoint): string {
  return JSON.stringify(bp, Object.keys(bp).sort());
}

/**
 * `edb_getBreakpointHits` takes a breakpoint and returns the snapshot ids
 * where that breakpoint would have triggered. The engine expects the
 * wire-format (externally-tagged) shape for `BreakpointLocation`; convert
 * our internal `kind`-tagged form via {@link breakpointToWire} before
 * sending.
 */
export function useBreakpointHits(bp: Breakpoint | null) {
  return useQuery({
    queryKey: ['bp-hits', bp ? stableHash(bp) : null] as const,
    queryFn: () => rpc('edb_getBreakpointHits', Schema, [breakpointToWire(bp!)]),
    enabled: bp !== null,
  });
}

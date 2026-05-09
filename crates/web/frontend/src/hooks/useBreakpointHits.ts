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
 *
 * Disabled breakpoints (`enabled === false`) skip the network roundtrip and
 * resolve to an empty hit set — UI consumers (Continue, jump-to-first)
 * treat them as if they didn't exist.
 */
export function useBreakpointHits(bp: Breakpoint | null) {
  const enabled = bp !== null && bp.enabled !== false;
  return useQuery({
    queryKey: ['bp-hits', bp ? stableHash(bp) : null] as const,
    queryFn: () =>
      enabled
        ? rpc('edb_getBreakpointHits', Schema, [breakpointToWire(bp!)])
        : Promise.resolve([] as number[]),
    enabled: bp !== null,
  });
}

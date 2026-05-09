import { useQueries, useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';
import { rpc } from '../lib/rpc';
import { CodeKind, Trace } from '../lib/types';
import { DISASM_PATH } from '../layout/FileTabPanel';

export interface AvailableFile {
  addr: string;
  path: string;
}

interface TraceEntry {
  id: number;
  kind: string;
  code_address: string;
  target_address: string;
  children?: TraceEntry[];
}

/** Hard cap on how many `edb_getCodeByAddress` queries run in parallel. */
const PARALLEL_CODE_QUERY_LIMIT = 25;

function collectAddresses(trace: TraceEntry[] | undefined): string[] {
  if (!trace) return [];
  const out = new Set<string>();
  const walk = (e: TraceEntry) => {
    if (e.code_address) out.add(e.code_address.toLowerCase());
    e.children?.forEach(walk);
  };
  trace.forEach(walk);
  return Array.from(out);
}

/**
 * Returns the union of every (addr, path) pair available in the loaded trace.
 * Uses cached `useQuery` calls so it shares cache with the file explorer.
 *
 * Concurrency is capped at {@link PARALLEL_CODE_QUERY_LIMIT}: only the first
 * N addresses (by trace order) are fetched eagerly. Later addresses still
 * have their query objects in place so the cache stays consistent, but the
 * `enabled` flag is `false` until earlier ones settle. Once any of the
 * leading queries lands in cache, the next batch unlocks automatically.
 */
export function useAvailableFiles(): {
  files: AvailableFile[];
  addresses: string[];
  isLoading: boolean;
} {
  const traceQ = useQuery({
    queryKey: ['trace'] as const,
    queryFn: () => rpc('edb_getTrace', Trace),
  });
  const addresses = useMemo(
    () => collectAddresses(traceQ.data as TraceEntry[] | undefined),
    [traceQ.data],
  );
  const codeQs = useQueries({
    queries: addresses.map((addr, i) => ({
      queryKey: ['code-addr', addr] as const,
      queryFn: () => rpc('edb_getCodeByAddress', CodeKind, [addr]),
      // Stagger fan-out: only the first N addresses fire eagerly; later
      // ones wait until they fall into the leading window.
      enabled: !!addr && i < PARALLEL_CODE_QUERY_LIMIT,
    })),
  });

  const files = useMemo<AvailableFile[]>(() => {
    const out: AvailableFile[] = [];
    addresses.forEach((addr, i) => {
      const data = codeQs[i]?.data;
      if (!data) return;
      if (data.kind === 'Opcodes') {
        out.push({ addr, path: DISASM_PATH });
        return;
      }
      data.files.forEach((f) => out.push({ addr, path: f.path }));
    });
    return out;
  }, [addresses, codeQs]);

  const isLoading = traceQ.isLoading || codeQs.some((q) => q.isLoading);
  return { files, addresses, isLoading };
}

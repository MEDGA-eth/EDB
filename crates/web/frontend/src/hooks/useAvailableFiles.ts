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
    queries: addresses.map((addr) => ({
      queryKey: ['code-addr', addr] as const,
      queryFn: () => rpc('edb_getCodeByAddress', CodeKind, [addr]),
      enabled: !!addr,
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

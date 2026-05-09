import { useMemo, useState } from 'react';
import { ChevronDown, ChevronRight, FileCode2 } from 'lucide-react';
import { useTrace } from '../../hooks/useTrace';
import { useCodeByAddress } from '../../hooks/useCodeByAddress';
import { useSession } from '../../store/session';
import { ErrorBoundary } from '../ErrorBoundary';
import { DISASM_PATH } from '../../layout/FileTabPanel';

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

function shortAddr(a: string): string {
  return `${a.slice(0, 6)}…${a.slice(-4)}`;
}

export function FileExplorer() {
  return (
    <ErrorBoundary label="FileExplorer">
      <FileExplorerInner />
    </ErrorBoundary>
  );
}

function FileExplorerInner() {
  const { data: trace, isLoading } = useTrace();
  const addresses = useMemo(() => collectAddresses(trace as TraceEntry[] | undefined), [trace]);

  if (isLoading)
    return (
      <div
        className="px-3 py-2 text-xs text-(--color-fg-tertiary)"
        data-testid="explorer-loading"
      >
        Loading…
      </div>
    );

  if (addresses.length === 0)
    return (
      <div className="px-3 py-2 text-xs text-(--color-fg-tertiary)" data-testid="explorer-empty">
        No contracts in trace.
      </div>
    );

  return (
    <ul className="font-display text-sm" data-testid="file-explorer">
      {addresses.map((addr) => (
        <AddressNode key={addr} addr={addr} />
      ))}
    </ul>
  );
}

function AddressNode({ addr }: { addr: string }) {
  const [expanded, setExpanded] = useState(true);
  const { data } = useCodeByAddress(addr);
  const open = useSession((s) => s.openFile);

  const Chevron = expanded ? ChevronDown : ChevronRight;

  const files: { path: string; label: string }[] = useMemo(() => {
    if (!data) return [];
    if (data.kind === 'Opcodes') return [{ path: DISASM_PATH, label: 'disasm' }];
    return data.files.map((f) => ({ path: f.path, label: f.path.split('/').pop() ?? f.path }));
  }, [data]);

  return (
    <li className="select-none">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="flex w-full items-center gap-1 px-2 py-1 text-left hover:bg-(--color-bg-hover)"
        data-testid={`explorer-addr-${addr}`}
        aria-label={`${expanded ? 'Collapse' : 'Expand'} ${shortAddr(addr)}`}
        aria-expanded={expanded}
      >
        <Chevron size={12} className="text-(--color-fg-tertiary)" />
        <span className="font-mono text-xs text-(--color-syn-type)">{shortAddr(addr)}</span>
      </button>
      {expanded && (
        <ul>
          {files.length === 0 && (
            <li className="px-6 py-1 text-xs text-(--color-fg-tertiary)">…</li>
          )}
          {files.map((f) => (
            <li key={f.path}>
              <button
                type="button"
                onClick={() => open({ addr, path: f.path })}
                className="flex w-full items-center gap-2 px-2 py-1 pl-6 text-left text-xs hover:bg-(--color-bg-hover)"
                data-testid={`explorer-file-${addr}-${f.path}`}
                aria-label={`Open ${f.label}`}
              >
                <FileCode2 size={12} className="text-(--color-fg-tertiary)" />
                <span className="truncate">{f.label}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </li>
  );
}

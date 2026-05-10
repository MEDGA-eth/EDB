import { openSearchPanel } from '@codemirror/search';
import { Bug, Code2, Copy, FileCode2, Hash, Locate, Search, WrapText } from 'lucide-react';
import { useEffect, useRef } from 'react';
import { useCodeByAddress } from '../hooks/useCodeByAddress';
import { useSnapshotInfo } from '../hooks/useSnapshotInfo';
import { useSolidityEditor } from '../hooks/useSolidityEditor';
import { ErrorBoundary } from '../components/ErrorBoundary';
import { ErrorCard } from '../components/ErrorCard';
import { pcLineIndex, tokenize } from '../lib/opcodeTokens';
import { Toolbar, ToolbarButton, ToolbarDivider } from '../components/Toolbar';
import { useSession } from '../store/session';

/** marker path used for the synthetic disassembly tab when an address has no Solidity */
export const DISASM_PATH = '<disasm>';

/** Strip the directory chain so the editor toolbar shows just the file name.
 *  Full path stays available via the wrapper's `title` attribute. */
function basename(p: string): string {
  if (!p) return '';
  if (p === DISASM_PATH) return p;
  const idx = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
  return idx === -1 ? p : p.slice(idx + 1);
}

/** `0xABCD…1234` style for compact metadata rendering. */
function shortAddr(addr: string): string {
  if (!addr) return '';
  if (addr.length <= 12) return addr;
  return `${addr.slice(0, 6)}…${addr.slice(-4)}`;
}

/** Dockview passes parameters as `params` on the panel-props object. */
export interface FileTabPanelParams {
  addr: string;
  path: string;
}

export function FileTabPanel(
  props: { params?: FileTabPanelParams } | { addr: string; path: string },
) {
  const params: FileTabPanelParams =
    'params' in props && props.params
      ? props.params
      : (props as { addr: string; path: string });
  return (
    <ErrorBoundary label="FileTabPanel">
      <FileTabPanelInner addr={params.addr} path={params.path} />
    </ErrorBoundary>
  );
}

function FileTabPanelInner({ addr, path }: { addr: string; path: string }) {
  const { data, isLoading, error, refetch } = useCodeByAddress(addr);

  if (isLoading)
    return (
      <div className="p-4 text-(--color-fg-tertiary)" data-testid="file-tab-loading">
        Loading…
      </div>
    );
  if (error) return <ErrorCard message={(error as Error).message} onRetry={() => refetch()} />;
  if (!data) return null;

  if (data.kind === 'Opcodes') return <OpcodesView addr={addr} disasm={data.disasm} />;
  return <SolidityView addr={addr} path={path} files={data.files} entry={data.entry} hasOpcodes={false} />;
}

function OpcodesView({ addr, disasm }: { addr: string; disasm: string }) {
  // Mirror CodePanel's OpcodesView: highlight the line whose PC matches
  // the current snapshot, but only if that snapshot is at THIS address.
  // Tabs at other addresses stay un-highlighted so the user knows the
  // active execution context isn't here.
  const currentSnapshotId = useSession((s) => s.currentSnapshotId);
  const { data: snap } = useSnapshotInfo(currentSnapshotId);
  const currentLine = (() => {
    if (!snap || snap.detail.kind !== 'Opcode') return -1;
    if (snap.bytecode_address.toLowerCase() !== addr.toLowerCase()) return -1;
    return pcLineIndex(disasm, snap.detail.pc);
  })();
  const lineRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (currentLine < 0) return;
    lineRef.current?.scrollIntoView({ block: 'center', behavior: 'auto' });
  }, [currentLine]);
  function revealCurrent() {
    lineRef.current?.scrollIntoView({ block: 'center', behavior: 'auto' });
  }
  // Pulse from the global Locate-Current button. Each bump re-runs the
  // scroll-into-view so the highlighted PC re-centers even if it was
  // already visible (or already scrolled offscreen by the user).
  const revealTick = useSession((s) => s.revealTick);
  useEffect(() => {
    if (revealTick === 0 || currentLine < 0) return;
    lineRef.current?.scrollIntoView({ block: 'center', behavior: 'auto' });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [revealTick]);
  return (
    <div className="flex h-full flex-col">
      <Toolbar testid="file-toolbar-opcodes">
        <ToolbarButton
          icon={Copy}
          label="Copy"
          showLabel
          testid="file-copy"
          onClick={() => {
            if (navigator.clipboard?.writeText) void navigator.clipboard.writeText(disasm);
          }}
        />
        <ToolbarButton
          icon={Locate}
          label="Reveal current PC"
          showLabel
          testid="file-reveal-current"
          onClick={revealCurrent}
          disabled={currentLine < 0}
        />
        <ToolbarDivider />
        <span
          className="ml-1 flex min-w-0 items-center gap-2 font-display text-[12px] text-(--color-fg-tertiary)"
          data-testid="file-toolbar-meta"
          title={`${addr}\n${DISASM_PATH}`}
        >
          <span className="shrink-0 rounded-full border border-(--color-border) bg-(--color-bg) px-2 py-0.5 font-mono text-[10.5px] text-(--color-fg-secondary)">
            {shortAddr(addr)}
          </span>
          <span className="min-w-0 truncate font-mono text-[11.5px]">{DISASM_PATH}</span>
        </span>
      </Toolbar>
      <pre
        data-testid="opcodes-view"
        className="flex-1 overflow-auto p-4 font-mono text-sm leading-relaxed"
      >
        {disasm.split('\n').map((line, i) => {
          const isCurrent = i === currentLine;
          return (
            <div
              key={i}
              ref={isCurrent ? lineRef : undefined}
              data-edb-current={isCurrent ? 'true' : undefined}
              className={isCurrent ? 'opcodes-current-line -mx-4 px-4' : undefined}
            >
              {tokenize(line).map((tok, j) => (
                <span key={j} className={`syn-${tok.kind}`}>
                  {tok.text}
                </span>
              ))}
            </div>
          );
        })}
      </pre>
    </div>
  );
}

function SolidityView({
  addr,
  path,
  files,
  entry,
}: {
  addr: string;
  path: string;
  files: { path: string; content: string }[];
  entry: string;
  hasOpcodes: boolean;
}) {
  const file =
    files.find((f) => f.path === path) ?? files.find((f) => f.path === entry) ?? files[0];
  const wordWrap = useSession((s) => s.wordWrap);
  const showLineNumbers = useSession((s) => s.showLineNumbers);
  const addBreakpoint = useSession((s) => s.addBreakpoint);
  const removeBreakpoint = useSession((s) => s.removeBreakpoint);
  const breakpoints = useSession((s) => s.breakpoints);
  // Subscribe to the active snapshot so we can highlight the current line
  // when (a) the snapshot is a Hook (source-level) snapshot, AND (b) it
  // belongs to this tab's (addr, path).
  const currentSnapshotId = useSession((s) => s.currentSnapshotId);
  const { data: currentSnap } = useSnapshotInfo(currentSnapshotId);
  const highlightLine = (() => {
    if (!currentSnap || !file) return undefined;
    if (currentSnap.detail.kind !== 'Hook') return undefined;
    if (currentSnap.bytecode_address.toLowerCase() !== addr.toLowerCase()) return undefined;
    if (currentSnap.detail.path !== file.path) return undefined;
    // offsets are byte-offsets into the source file content; convert to line.
    const offset = Math.max(0, Math.min(currentSnap.detail.offset, file.content.length));
    // Count newlines up to offset, +1 because lines are 1-indexed.
    let line = 1;
    for (let i = 0; i < offset; i += 1) {
      if (file.content.charCodeAt(i) === 10) line += 1;
    }
    return line;
  })();
  // Filter the global breakpoint list down to source breakpoints that match
  // this editor's (addr, path). The marker list is intentionally small,
  // typically <10, so an O(n) scan per render is fine.
  const bpMarkers = (() => {
    if (!file) return [];
    const matching: { line: number; enabled: boolean }[] = [];
    for (const bp of breakpoints) {
      if (!bp.loc || bp.loc.kind !== 'Source') continue;
      if (bp.loc.bytecode_address.toLowerCase() !== addr.toLowerCase()) continue;
      if (bp.loc.file_path !== file.path) continue;
      matching.push({ line: bp.loc.line_number, enabled: bp.enabled ?? true });
    }
    return matching;
  })();
  function toggleBreakpointAtLine(line: number) {
    if (!file) return;
    const idx = breakpoints.findIndex(
      (bp) =>
        bp.loc &&
        bp.loc.kind === 'Source' &&
        bp.loc.bytecode_address.toLowerCase() === addr.toLowerCase() &&
        bp.loc.file_path === file.path &&
        bp.loc.line_number === line,
    );
    if (idx >= 0) {
      removeBreakpoint(idx);
    } else {
      addBreakpoint({
        loc: { kind: 'Source', bytecode_address: addr, file_path: file.path, line_number: line },
        condition: null,
      });
    }
  }
  const { containerRef, viewRef, revealOffset } = useSolidityEditor({
    content: file?.content ?? '',
    wordWrap,
    showLineNumbers,
    highlightLine,
    breakpoints: bpMarkers,
    onToggleBreakpoint: toggleBreakpointAtLine,
  });

  function revealCurrent() {
    if (!currentSnap) return;
    if (currentSnap.detail.kind === 'Hook') {
      revealOffset(currentSnap.detail.offset);
    }
  }

  // Global Locate-Current pulse. Re-scroll on every bump as long as the
  // active snapshot lands in this (addr, path); otherwise stay put — the
  // matching tab elsewhere handles the reveal.
  const revealTick = useSession((s) => s.revealTick);
  useEffect(() => {
    if (revealTick === 0) return;
    if (!currentSnap || currentSnap.detail.kind !== 'Hook') return;
    if (currentSnap.bytecode_address.toLowerCase() !== addr.toLowerCase()) return;
    if (currentSnap.detail.path !== file?.path) return;
    revealOffset(currentSnap.detail.offset);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [revealTick]);

  function copyContent() {
    if (!file) return;
    if (navigator.clipboard?.writeText) void navigator.clipboard.writeText(file.content);
  }
  function findInFile() {
    const view = viewRef.current;
    if (view) openSearchPanel(view);
  }
  function addBreakpointAtCursor() {
    const view = viewRef.current;
    if (!view || !file) return;
    // CodeMirror 'select-line' (or any forward range selection that ends at a
    // line break) puts `head` at the START of the next line, which causes
    // lineAt(head) to return that next line, off by one in the user's eyes.
    // Use `from` (the lower edge of the range) so selecting a whole line
    // sets the breakpoint on THAT line, not the one below it.
    const sel = view.state.selection.main;
    const pos = sel.from;
    const line = view.state.doc.lineAt(pos).number;
    addBreakpoint({
      loc: { kind: 'Source', bytecode_address: addr, file_path: file.path, line_number: line },
      condition: null,
    });
  }

  return (
    <div className="flex h-full flex-col">
      <Toolbar testid="file-toolbar-source">
        <ToolbarButton
          icon={Search}
          label="Find"
          showLabel
          testid="file-find"
          onClick={findInFile}
        />
        <ToolbarButton
          icon={Copy}
          label="Copy"
          showLabel
          testid="file-copy"
          onClick={copyContent}
        />
        <ToolbarDivider />
        <ToolbarButton
          icon={WrapText}
          label="Wrap"
          showLabel
          testid="file-wrap"
          active={wordWrap}
          onClick={() => useSession.getState().toggleWordWrap()}
        />
        <ToolbarButton
          icon={Hash}
          label="Line numbers"
          showLabel
          testid="file-line-numbers"
          active={showLineNumbers}
          onClick={() => useSession.getState().toggleLineNumbers()}
        />
        <ToolbarDivider />
        <ToolbarButton
          icon={Bug}
          label="Breakpoint here"
          showLabel
          testid="file-add-breakpoint"
          onClick={addBreakpointAtCursor}
        />
        <ToolbarButton
          icon={Locate}
          label="Reveal current"
          showLabel
          testid="file-reveal-current"
          onClick={revealCurrent}
          disabled={typeof highlightLine !== 'number'}
        />
        <ToolbarDivider />
        {/* Compact source location. Long paths used to overflow the toolbar
            (`/Users/.../FiatTokenV2_2.sol`); we now show address + filename
            with the full path + full address available via `title`. */}
        <span
          className="ml-1 flex min-w-0 items-center gap-2 font-display text-[12px] text-(--color-fg-tertiary)"
          data-testid="file-toolbar-meta"
          title={`${addr}\n${file?.path ?? ''}`}
        >
          <span className="shrink-0 rounded-full border border-(--color-border) bg-(--color-bg) px-2 py-0.5 font-mono text-[10.5px] text-(--color-fg-secondary)">
            {shortAddr(addr)}
          </span>
          <FileCode2 size={12} aria-hidden className="shrink-0 text-(--color-syn-type-std)" />
          <span className="min-w-0 truncate font-mono text-[11.5px] font-semibold text-(--color-fg-secondary)">
            {basename(file?.path ?? '')}
          </span>
          <Code2 size={12} aria-hidden className="shrink-0" />
        </span>
      </Toolbar>
      <div ref={containerRef} data-testid="solidity-view" className="flex-1 overflow-auto" />
    </div>
  );
}

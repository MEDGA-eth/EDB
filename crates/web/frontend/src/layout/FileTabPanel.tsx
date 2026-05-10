import { openSearchPanel } from '@codemirror/search';
import { Bug, Code2, Copy, FileCode2, Hash, Locate, Search, WrapText } from 'lucide-react';
import { useCodeByAddress } from '../hooks/useCodeByAddress';
import { useSnapshotInfo } from '../hooks/useSnapshotInfo';
import { useSolidityEditor } from '../hooks/useSolidityEditor';
import { ErrorBoundary } from '../components/ErrorBoundary';
import { ErrorCard } from '../components/ErrorCard';
import { tokenize } from '../lib/opcodeTokens';
import { Toolbar, ToolbarButton, ToolbarDivider } from '../components/Toolbar';
import { useSession } from '../store/session';

/** marker path used for the synthetic disassembly tab when an address has no Solidity */
export const DISASM_PATH = '<disasm>';

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
        <ToolbarDivider />
        <span
          className="font-display text-[12px] text-(--color-fg-tertiary)"
          data-testid="file-toolbar-meta"
        >
          {addr.slice(0, 10)}… · {DISASM_PATH}
        </span>
      </Toolbar>
      <pre
        data-testid="opcodes-view"
        className="flex-1 overflow-auto p-4 font-mono text-sm leading-relaxed"
      >
        {disasm.split('\n').map((line, i) => (
          <div key={i}>
            {tokenize(line).map((tok, j) => (
              <span key={j} className={`syn-${tok.kind}`}>
                {tok.text}
              </span>
            ))}
          </div>
        ))}
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
        <span
          className="ml-1 inline-flex items-center gap-1 font-display text-[12px] text-(--color-fg-tertiary)"
          data-testid="file-toolbar-meta"
        >
          <FileCode2 size={12} aria-hidden />
          {addr.slice(0, 10)}… · {file?.path ?? ''}
          <Code2 size={12} aria-hidden />
        </span>
      </Toolbar>
      <div ref={containerRef} data-testid="solidity-view" className="flex-1 overflow-auto" />
    </div>
  );
}

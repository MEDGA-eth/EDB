import { useEffect, useRef } from 'react';
import {
  DockviewReact,
  type DockviewApi,
  type DockviewReadyEvent,
  type IDockviewPanelProps,
} from 'dockview';
import { ActivityBar } from './ActivityBar';
import { StatusBar } from './StatusBar';
import { FileTabPanel, type FileTabPanelParams } from './FileTabPanel';
import { TracePanel } from '../components/panels/TracePanel';
import { DisplayPanel } from '../components/panels/DisplayPanel';
import { TerminalPanel } from '../components/panels/TerminalPanel';
import { FileExplorer } from '../components/explorer/FileExplorer';
import { BreakpointsView } from '../components/explorer/BreakpointsView';
import { CommandPalette } from '../components/CommandPalette';
import { useGlobalKeybinds } from '../hooks/useGlobalKeybinds';
import { useSnapshotCount } from '../hooks/useSnapshotCount';
import { useTrace } from '../hooks/useTrace';
import { useSession, type ActivityKind } from '../store/session';
import {
  ACTIVITY_WIDTH_PX,
  LAYOUT_KEY,
  LAYOUT_VERSION,
  SIDEBAR_WIDTH_PX,
} from '../lib/constants';

/** Subset of dockview's `IDisposable` we care about. */
interface Disposable {
  dispose(): void;
}

const BOTTOM_PANEL_IDS = new Set(['trace', 'display', 'terminal']);

/* ── editor (file-tab) area ───────────────────────────────── */

const editorComponents: Record<string, React.FunctionComponent<IDockviewPanelProps>> = {
  file: (props: IDockviewPanelProps<FileTabPanelParams>) => <FileTabPanel params={props.params} />,
};

function EditorArea() {
  const apiRef = useRef<DockviewApi | null>(null);
  const disposablesRef = useRef<Disposable[]>([]);
  const openFiles = useSession((s) => s.openFiles);
  const activeFileId = useSession((s) => s.activeFileId);
  const setActiveFile = useSession((s) => s.setActiveFile);
  const closeFile = useSession((s) => s.closeFile);

  const onReady = (event: DockviewReadyEvent) => {
    apiRef.current = event.api;
    // mirror tab-activations + closes back into the store. Capture the
    // returned disposables so we can detach them on unmount (otherwise
    // StrictMode-induced double-mounts in dev double-subscribe).
    disposablesRef.current.push(
      event.api.onDidActivePanelChange((p) => {
        if (p) setActiveFile(p.id);
      }),
      event.api.onDidRemovePanel((p) => {
        // if dockview removed it via the close button, drop from store too
        const stillOpen = useSession.getState().openFiles.some((f) => f.id === p.id);
        if (stillOpen) closeFile(p.id);
      }),
    );
    // Seed dockview with whatever the store already had open. The reconcile
    // useEffect runs once before `onReady` (mounts before paint), so it
    // misses the very first transition from `openFiles=[]` to `[file]` —
    // hence we replay the set here when the api becomes available.
    const wantedFiles = useSession.getState().openFiles;
    for (const f of wantedFiles) {
      if (!event.api.getPanel(f.id)) {
        event.api.addPanel({
          id: f.id,
          component: 'file',
          title: f.path === '<disasm>' ? 'disasm' : (f.path.split('/').pop() ?? f.path),
          params: { addr: f.addr, path: f.path },
        });
      }
    }
    const activeId = useSession.getState().activeFileId;
    if (activeId) event.api.getPanel(activeId)?.api.setActive();
  };

  // Dispose dockview event subscriptions when the EditorArea unmounts.
  useEffect(() => {
    return () => {
      for (const d of disposablesRef.current) {
        try {
          d.dispose();
        } catch {
          // ignore
        }
      }
      disposablesRef.current = [];
    };
  }, []);

  // reconcile dockview panels with store openFiles
  useEffect(() => {
    const api = apiRef.current;
    if (!api) return;

    const wantedIds = new Set(openFiles.map((f) => f.id));
    const current = api.panels.filter((p) => !BOTTOM_PANEL_IDS.has(p.id));

    // remove panels that the store no longer has
    for (const p of current) {
      if (!wantedIds.has(p.id)) api.removePanel(p);
    }
    // add panels that are missing
    for (const f of openFiles) {
      if (!api.getPanel(f.id)) {
        api.addPanel({
          id: f.id,
          component: 'file',
          title: f.path === '<disasm>' ? 'disasm' : (f.path.split('/').pop() ?? f.path),
          params: { addr: f.addr, path: f.path },
        });
      }
    }
  }, [openFiles]);

  // sync active panel when activeFileId changes externally
  useEffect(() => {
    const api = apiRef.current;
    if (!api || !activeFileId) return;
    const panel = api.getPanel(activeFileId);
    if (panel && api.activePanel?.id !== panel.id) panel.api.setActive();
  }, [activeFileId]);

  return (
    <div className="h-full w-full" data-testid="editor-area">
      {openFiles.length === 0 ? (
        <EditorEmpty />
      ) : (
        <DockviewReact
          components={editorComponents}
          onReady={onReady}
          className="dockview-theme-edb h-full w-full"
        />
      )}
    </div>
  );
}

function EditorEmpty() {
  return (
    <div
      className="flex h-full w-full items-center justify-center bg-(--color-bg-elevated) text-(--color-fg-tertiary)"
      data-testid="editor-empty"
    >
      <div className="text-center">
        <div className="font-display text-sm font-semibold">edb</div>
        <div className="mt-1 text-xs">Pick a contract from the Explorer to start.</div>
      </div>
    </div>
  );
}

/* ── bottom panel area ────────────────────────────────────── */

const bottomComponents: Record<string, React.FunctionComponent<IDockviewPanelProps>> = {
  trace: () => <TracePanel />,
  display: () => <DisplayPanel />,
  terminal: () => <TerminalPanel />,
};

function BottomArea() {
  const apiRef = useRef<DockviewApi | null>(null);
  const disposablesRef = useRef<Disposable[]>([]);
  const setLayoutJson = useSession((s) => s.setLayoutJson);

  const onReady = (event: DockviewReadyEvent) => {
    const api = event.api;
    apiRef.current = api;
    // try to restore from session/localStorage. Persisted state is wrapped
    // as `{ version, layout }`; mismatched versions are dropped so older
    // dockview JSON shapes can never wedge a fresh build.
    const saved = useSession.getState().layoutJson ?? localStorage.getItem(LAYOUT_KEY);
    let restored = false;
    if (saved) {
      try {
        const parsed = JSON.parse(saved) as { version?: number; layout?: unknown };
        if (parsed && parsed.version === LAYOUT_VERSION && parsed.layout) {
          api.fromJSON(parsed.layout as Parameters<typeof api.fromJSON>[0]);
          restored = true;
        }
      } catch {
        // fall through to default
      }
    }
    if (!restored) {
      // Idempotent: under React 19 StrictMode, `onReady` runs twice in dev.
      // Skip any default panel that already exists from the previous mount.
      if (!api.getPanel('trace')) {
        api.addPanel({ id: 'trace', component: 'trace', title: 'Trace' });
      }
      if (!api.getPanel('display')) {
        api.addPanel({
          id: 'display',
          component: 'display',
          title: 'Display',
          position: { referencePanel: 'trace', direction: 'within' },
        });
      }
      if (!api.getPanel('terminal')) {
        api.addPanel({
          id: 'terminal',
          component: 'terminal',
          title: 'Terminal',
          position: { referencePanel: 'trace', direction: 'within' },
        });
      }
      // make Trace the visible tab
      api.getPanel('trace')?.api.setActive();
    }

    // Subscribe AFTER default-panel setup completes so the partial-init
    // states emitted during `addPanel` calls aren't persisted as the
    // "saved" layout. Otherwise a refresh mid-init could pin a half-built
    // shape into localStorage.
    disposablesRef.current.push(
      api.onDidLayoutChange(() => {
        try {
          const json = JSON.stringify({ version: LAYOUT_VERSION, layout: api.toJSON() });
          setLayoutJson(json);
          localStorage.setItem(LAYOUT_KEY, json);
        } catch {
          // ignore
        }
      }),
    );
  };

  // Dispose dockview event subscriptions when BottomArea unmounts.
  useEffect(() => {
    return () => {
      for (const d of disposablesRef.current) {
        try {
          d.dispose();
        } catch {
          // ignore
        }
      }
      disposablesRef.current = [];
    };
  }, []);

  return (
    <DockviewReact
      components={bottomComponents}
      onReady={onReady}
      className="dockview-theme-edb h-full w-full"
    />
  );
}

/* ── side bar ─────────────────────────────────────────────── */

function SideBar({ activity }: { activity: ActivityKind }) {
  return (
    <aside
      className="flex h-full flex-col overflow-hidden border-r border-(--color-border) bg-(--color-bg)"
      style={{ width: SIDEBAR_WIDTH_PX }}
      data-testid="side-bar"
    >
      <header className="flex h-8 items-center border-b border-(--color-border) px-3 font-display text-[11px] font-semibold tracking-wide text-(--color-fg-secondary) uppercase">
        {activity}
      </header>
      <div className="flex-1 overflow-auto" data-testid={`side-bar-${activity}`}>
        {activity === 'explorer' ? (
          <FileExplorer />
        ) : activity === 'breakpoints' ? (
          <BreakpointsView />
        ) : (
          <SidePlaceholder activity={activity} />
        )}
      </div>
    </aside>
  );
}

function SidePlaceholder({ activity }: { activity: ActivityKind }) {
  return (
    <div className="px-3 py-3 text-xs text-(--color-fg-tertiary)">
      {activity} panel — coming soon.
    </div>
  );
}

/* ── shell ────────────────────────────────────────────────── */

/** Walk a trace tree and collect all `code_address` strings (lower-cased). */
function collectAddressesFromTrace(trace: unknown): Set<string> {
  const out = new Set<string>();
  if (!Array.isArray(trace)) return out;
  type Entry = { code_address?: string; children?: Entry[] };
  const walk = (e: Entry) => {
    if (typeof e.code_address === 'string') out.add(e.code_address.toLowerCase());
    e.children?.forEach(walk);
  };
  (trace as Entry[]).forEach(walk);
  return out;
}

export function IDELayout() {
  const activity = useSession((s) => s.activeActivity);
  useGlobalKeybinds();

  // Close any open file whose address is no longer in the loaded trace.
  // Keyed on `data` reference identity so we only run when the trace query
  // actually produces a new value (not on every IDELayout render).
  const trace = useTrace();
  useEffect(() => {
    const data = trace.data;
    if (!data) return;
    const addrs = collectAddressesFromTrace(data);
    const open = useSession.getState().openFiles;
    const close = useSession.getState().closeFile;
    for (const f of open) {
      if (!addrs.has(f.addr.toLowerCase())) close(f.id);
    }
  }, [trace.data]);

  // Show a top-level banner when both queries succeed but return empty data.
  // Loading states are excluded so the banner doesn't flash during cold start.
  const snapshotCountQ = useSnapshotCount();
  const traceIsEmpty = trace.isSuccess && Array.isArray(trace.data) && trace.data.length === 0;
  const noSnapshots = snapshotCountQ.isSuccess && snapshotCountQ.data === 0;
  const showEmptyBanner = traceIsEmpty && noSnapshots;

  return (
    <div className="flex h-full flex-col bg-(--color-bg-root)" data-testid="ide-layout">
      {showEmptyBanner && (
        <div
          data-testid="empty-trace-banner"
          role="status"
          className="flex items-center justify-center border-b border-(--color-border) bg-(--color-bg-elevated) px-4 py-2 font-display text-xs text-(--color-fg-secondary)"
        >
          No trace loaded — start with{' '}
          <code className="mx-1 font-mono">edb replay --ui=web &lt;tx&gt;</code>
        </div>
      )}
      <div className="flex flex-1 overflow-hidden">
        {/* activity bar (left) */}
        <div style={{ width: ACTIVITY_WIDTH_PX }} className="shrink-0">
          <ActivityBar />
        </div>
        {/* sidebar */}
        <SideBar activity={activity} />
        {/* main split: editor (top) + bottom panel */}
        <div className="flex flex-1 flex-col overflow-hidden">
          <div className="flex-1 overflow-hidden" data-testid="editor-region">
            {/*
              The editor area stays mounted across window resizes — dockview
              observes its container and handles its own layout. Re-mounting
              on every resize was pathological under drag (60fps).
            */}
            <EditorArea />
          </div>
          <div
            className="h-2 cursor-row-resize border-t border-b border-(--color-border) bg-(--color-bg)"
            aria-hidden
          />
          <div className="h-[40%] min-h-[160px] overflow-hidden" data-testid="bottom-region">
            <BottomArea />
          </div>
        </div>
      </div>
      <StatusBar />
      <CommandPalette />
    </div>
  );
}

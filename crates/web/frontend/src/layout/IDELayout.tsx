import { useEffect, useRef, useState } from 'react';
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
import { useSession, type ActivityKind } from '../store/session';

const SIDEBAR_WIDTH = 280;
const ACTIVITY_WIDTH = 48;
const LAYOUT_KEY = 'edb-web:layout';

const BOTTOM_PANEL_IDS = new Set(['trace', 'display', 'terminal']);

/* ── editor (file-tab) area ───────────────────────────────── */

const editorComponents: Record<string, React.FunctionComponent<IDockviewPanelProps>> = {
  file: (props: IDockviewPanelProps<FileTabPanelParams>) => <FileTabPanel params={props.params} />,
};

function EditorArea() {
  const apiRef = useRef<DockviewApi | null>(null);
  const openFiles = useSession((s) => s.openFiles);
  const activeFileId = useSession((s) => s.activeFileId);
  const setActiveFile = useSession((s) => s.setActiveFile);
  const closeFile = useSession((s) => s.closeFile);

  const onReady = (event: DockviewReadyEvent) => {
    apiRef.current = event.api;
    // mirror tab-activations + closes back into the store
    event.api.onDidActivePanelChange((p) => {
      if (p) setActiveFile(p.id);
    });
    event.api.onDidRemovePanel((p) => {
      // if dockview removed it via the close button, drop from store too
      const stillOpen = useSession.getState().openFiles.some((f) => f.id === p.id);
      if (stillOpen) closeFile(p.id);
    });
  };

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
  const setLayoutJson = useSession((s) => s.setLayoutJson);

  const onReady = (event: DockviewReadyEvent) => {
    const api = event.api;
    apiRef.current = api;
    // try to restore from session/localStorage
    const saved = useSession.getState().layoutJson ?? localStorage.getItem(LAYOUT_KEY);
    let restored = false;
    if (saved) {
      try {
        api.fromJSON(JSON.parse(saved));
        restored = true;
      } catch {
        // fall through to default
      }
    }
    if (!restored) {
      api.addPanel({ id: 'trace', component: 'trace', title: 'Trace' });
      api.addPanel({
        id: 'display',
        component: 'display',
        title: 'Display',
        position: { referencePanel: 'trace', direction: 'within' },
      });
      api.addPanel({
        id: 'terminal',
        component: 'terminal',
        title: 'Terminal',
        position: { referencePanel: 'trace', direction: 'within' },
      });
      // make Trace the visible tab
      api.getPanel('trace')?.api.setActive();
    }

    api.onDidLayoutChange(() => {
      try {
        const json = JSON.stringify(api.toJSON());
        setLayoutJson(json);
        localStorage.setItem(LAYOUT_KEY, json);
      } catch {
        // ignore
      }
    });
  };

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
      style={{ width: SIDEBAR_WIDTH }}
      data-testid="side-bar"
    >
      <header className="flex h-8 items-center border-b border-(--color-border) px-3 font-display text-[11px] font-semibold tracking-wide text-(--color-fg-secondary) uppercase">
        {activity}
      </header>
      <div className="flex-1 overflow-auto" data-testid={`side-bar-${activity}`}>
        {activity === 'explorer' ? <FileExplorer /> : <SidePlaceholder activity={activity} />}
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

export function IDELayout() {
  const activity = useSession((s) => s.activeActivity);
  const [editorPx, setEditorPx] = useState(0); // forces re-mount when window resizes radically

  useEffect(() => {
    const onResize = () => setEditorPx((n) => n + 1);
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);

  return (
    <div className="flex h-full flex-col bg-(--color-bg-root)" data-testid="ide-layout">
      <div className="flex flex-1 overflow-hidden">
        {/* activity bar (left) */}
        <div style={{ width: ACTIVITY_WIDTH }} className="shrink-0">
          <ActivityBar />
        </div>
        {/* sidebar */}
        <SideBar activity={activity} />
        {/* main split: editor (top) + bottom panel */}
        <div className="flex flex-1 flex-col overflow-hidden">
          <div className="flex-1 overflow-hidden" data-testid="editor-region">
            {/* `key` lets the empty-state swap to dockview cleanly */}
            <EditorArea key={editorPx} />
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
    </div>
  );
}

import { useEffect, useRef } from 'react';
import {
  DockviewReact,
  type DockviewApi,
  type DockviewReadyEvent,
  type IDockviewPanelProps,
} from 'dockview';
import { FileTabPanel, type FileTabPanelParams } from './FileTabPanel';
import { useSession } from '../store/session';

/** Subset of dockview's `IDisposable` we care about. */
interface Disposable {
  dispose(): void;
}

const BOTTOM_PANEL_IDS = new Set(['trace', 'display', 'terminal']);

const editorComponents: Record<string, React.FunctionComponent<IDockviewPanelProps>> = {
  file: (props: IDockviewPanelProps<FileTabPanelParams>) => <FileTabPanel params={props.params} />,
};

/** Title shown on a file tab. Centralises the `<disasm>` special-case. */
function tabTitle(path: string): string {
  return path === '<disasm>' ? '(disassembly)' : (path.split('/').pop() ?? path);
}

export function EditorArea() {
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
          title: tabTitle(f.path),
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
          title: tabTitle(f.path),
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

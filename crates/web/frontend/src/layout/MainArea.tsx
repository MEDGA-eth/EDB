import { useEffect, useRef } from 'react';
import {
  DockviewReact,
  type DockviewApi,
  type DockviewReadyEvent,
  type IDockviewPanelProps,
} from 'dockview';
import { FileTabPanel, DISASM_PATH, type FileTabPanelParams } from './FileTabPanel';
import { DisplayPanel } from '../components/panels/DisplayPanel';
import { TerminalPanel } from '../components/panels/TerminalPanel';
import { useSession } from '../store/session';
import { LAYOUT_KEY, LAYOUT_VERSION } from '../lib/constants';
import { setDockviewApi } from '../lib/dockviewBridge';
import { useSnapshotInfo } from '../hooks/useSnapshotInfo';

interface Disposable {
  dispose(): void;
}

/** Panel ids that are NOT file tabs, used by the reconcile loop to skip
 *  pruning the bottom panels when the open-file set changes. */
const FIXED_PANEL_IDS = new Set(['display', 'terminal']);

const components: Record<string, React.FunctionComponent<IDockviewPanelProps>> = {
  file: (props: IDockviewPanelProps<FileTabPanelParams>) => (
    <FileTabPanel params={props.params} />
  ),
  display: () => <DisplayPanel />,
  terminal: () => <TerminalPanel />,
};

/** Title shown on a file tab. Centralises the `<disasm>` special-case. */
function tabTitle(path: string): string {
  return path === '<disasm>' ? '(disassembly)' : (path.split('/').pop() ?? path);
}

/**
 * The whole working area as a single DockviewReact instance. Combining
 * file tabs and the bottom panels into one dockview lets users drag
 * Terminal / Display tabs anywhere, e.g. side-by-side with the code
 * editor, exactly the way VSCode allows. The previous design used two
 * separate DockviewReact instances which prevented cross-region drops.
 */
export function MainArea() {
  const apiRef = useRef<DockviewApi | null>(null);
  const disposablesRef = useRef<Disposable[]>([]);
  const setLayoutJson = useSession((s) => s.setLayoutJson);
  const openFiles = useSession((s) => s.openFiles);
  const activeFileId = useSession((s) => s.activeFileId);
  const setActiveFile = useSession((s) => s.setActiveFile);
  const closeFile = useSession((s) => s.closeFile);
  // Watch the current snapshot too: stepping within a single file keeps
  // `activeFileId` stable (same `${addr}::${path}` key), so a useEffect
  // keyed only on activeFileId would never re-fire and the file panel
  // would stay buried behind whatever tab the user clicked last.
  const currentSnapshotId = useSession((s) => s.currentSnapshotId);

  const onReady = (event: DockviewReadyEvent) => {
    const api = event.api;
    apiRef.current = api;
    setDockviewApi(api);

    // Restore from persisted layout if the schema version matches. We
    // serialise the same `{version, layout}` envelope the old BottomArea
    // wrote, and bump LAYOUT_VERSION when the panel topology changes.
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
        // fall through to defaults
      }
    }

    if (!restored) {
      // Default topology: a bottom group with Display + Terminal, and a
      // top group with whatever files the store has open (initially empty,
      // and dockview's watermark will fill the space). Files added later
      // join the existing top group via `direction: 'within'` against any
      // sibling file panel, see the reconcile useEffect below.
      // Skipped panels when StrictMode replays this in dev.
      if (!api.getPanel('display')) {
        api.addPanel({ id: 'display', component: 'display', title: 'Display' });
      }
      if (!api.getPanel('terminal')) {
        api.addPanel({
          id: 'terminal',
          component: 'terminal',
          title: 'Terminal',
          position: { referencePanel: 'display', direction: 'within' },
        });
      }
      const wantedFiles = useSession.getState().openFiles;
      let firstFileId: string | null = null;
      for (const f of wantedFiles) {
        if (!api.getPanel(f.id)) {
          // First file: create a new group ABOVE the display group. Later
          // files: join the file group via `within` against the prior tab.
          const position = firstFileId
            ? { referencePanel: firstFileId, direction: 'within' as const }
            : { referencePanel: 'display', direction: 'above' as const };
          api.addPanel({
            id: f.id,
            component: 'file',
            title: tabTitle(f.path),
            params: { addr: f.addr, path: f.path },
            position,
          });
          if (!firstFileId) firstFileId = f.id;
        }
      }
      api.getPanel('display')?.api.setActive();
      const activeId = useSession.getState().activeFileId;
      if (activeId) api.getPanel(activeId)?.api.setActive();
    }

    disposablesRef.current.push(
      api.onDidActivePanelChange((p) => {
        if (p && !FIXED_PANEL_IDS.has(p.id)) setActiveFile(p.id);
      }),
      api.onDidRemovePanel((p) => {
        if (FIXED_PANEL_IDS.has(p.id)) return;
        const stillOpen = useSession.getState().openFiles.some((f) => f.id === p.id);
        if (stillOpen) closeFile(p.id);
      }),
      api.onDidLayoutChange(() => {
        let json: string;
        try {
          json = JSON.stringify({ version: LAYOUT_VERSION, layout: api.toJSON() });
        } catch (e) {
          // eslint-disable-next-line no-console
          console.warn('[edb-web] failed to serialise dockview layout', e);
          return;
        }
        setLayoutJson(json);
        try {
          localStorage.setItem(LAYOUT_KEY, json);
        } catch (e) {
          // eslint-disable-next-line no-console
          console.warn('[edb-web] failed to persist dockview layout', e);
        }
      }),
    );
  };

  useEffect(() => {
    return () => {
      for (const d of disposablesRef.current) {
        try { d.dispose(); } catch { /* ignore */ }
      }
      disposablesRef.current = [];
      setDockviewApi(null);
    };
  }, []);

  // Reconcile dockview file panels with store openFiles. New file panels
  // join the existing file group via `direction: 'within'` against an
  // already-open file tab; if no files are open, place the new tab ABOVE
  // the bottom (display/terminal) group so the IDE keeps its code-on-top
  // / panels-on-bottom shape.
  //
  // Care: dockview throws if `referencePanel` doesn't exist. The user may
  // have closed `display` AND `terminal`, leaving the dockview empty,
  // anchoring to a missing id would crash the app. Always pick an anchor
  // that's actually present, or fall through to no `position` so dockview
  // makes a fresh group.
  useEffect(() => {
    const api = apiRef.current;
    if (!api) return;
    const wantedIds = new Set(openFiles.map((f) => f.id));
    const fileTabs = api.panels.filter((p) => !FIXED_PANEL_IDS.has(p.id));
    for (const p of fileTabs) {
      if (!wantedIds.has(p.id)) api.removePanel(p);
    }
    for (const f of openFiles) {
      if (api.getPanel(f.id)) continue;
      const existingFile = api.panels.find((p) => !FIXED_PANEL_IDS.has(p.id));
      const display = api.getPanel('display');
      const anyFixed = display ?? api.getPanel('terminal');
      const position = existingFile
        ? { referencePanel: existingFile.id, direction: 'within' as const }
        : anyFixed
          ? { referencePanel: anyFixed.id, direction: 'above' as const }
          : undefined;
      api.addPanel({
        id: f.id,
        component: 'file',
        title: tabTitle(f.path),
        params: { addr: f.addr, path: f.path },
        position,
      });
    }
  }, [openFiles]);

  // Reflect external `activeFileId` writes (URL hash, palette navigation,
  // step-driven follow). Also re-runs when the snapshot id changes, even
  // if the active file is unchanged, the user expects each Step to surface
  // the code view if they were sitting on Display / Terminal.
  useEffect(() => {
    const api = apiRef.current;
    if (!api || !activeFileId) return;
    const panel = api.getPanel(activeFileId);
    if (!panel) return;
    panel.api.setActive();
    // setActive on the panel also makes its group active in dockview, so
    // the code view scrolls to the front of its split. Calling it
    // unconditionally (not gated on `activePanel?.id !== panel.id`) is
    // what brings a buried tab back into view when the user has clicked
    // Display / Terminal between steps.
  }, [activeFileId, currentSnapshotId]);

  // Auto-follow: every snapshot change opens (or focuses) the file/disasm
  // tab matching the new (bytecode_address, path). Without this, stepping
  // into a contract whose tab isn't open just removes the highlight from
  // the currently-visible tab and the user "loses track". We key the
  // effect on the snap data itself rather than just `currentSnapshotId`
  // so the follow also fires when the snapshot was navigated to before
  // its data arrived (subsequent fetch resolution re-runs the effect).
  const { data: currentSnap } = useSnapshotInfo(currentSnapshotId);
  useEffect(() => {
    if (!currentSnap) return;
    const addr = currentSnap.bytecode_address;
    const path =
      currentSnap.detail.kind === 'Hook' ? currentSnap.detail.path : DISASM_PATH;
    const targetId = `${addr}::${path}`;
    if (useSession.getState().activeFileId === targetId) {
      // Already on the right tab; just re-pulse so the view re-centers.
      useSession.getState().bumpRevealTick();
      return;
    }
    useSession.getState().openFile({ addr, path });
    useSession.getState().bumpRevealTick();
  }, [currentSnap]);

  return (
    <div className="h-full w-full" data-testid="main-area">
      <DockviewReact
        components={components}
        onReady={onReady}
        className="dockview-theme-edb h-full w-full"
      />
    </div>
  );
}

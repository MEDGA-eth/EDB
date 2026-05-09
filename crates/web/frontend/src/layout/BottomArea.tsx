import { useEffect, useRef } from 'react';
import {
  DockviewReact,
  type DockviewApi,
  type DockviewReadyEvent,
  type IDockviewPanelProps,
} from 'dockview';
import { TracePanel } from '../components/panels/TracePanel';
import { DisplayPanel } from '../components/panels/DisplayPanel';
import { TerminalPanel } from '../components/panels/TerminalPanel';
import { useSession } from '../store/session';
import { LAYOUT_KEY, LAYOUT_VERSION } from '../lib/constants';

interface Disposable {
  dispose(): void;
}

const bottomComponents: Record<string, React.FunctionComponent<IDockviewPanelProps>> = {
  trace: () => <TracePanel />,
  display: () => <DisplayPanel />,
  terminal: () => <TerminalPanel />,
};

export function BottomArea() {
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

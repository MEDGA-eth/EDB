import type { ReactElement } from 'react';
import { useSession } from '../store/session';
import { CodePanel } from '../components/panels/CodePanel';
import { TracePanel } from '../components/panels/TracePanel';
import { DisplayPanel } from '../components/panels/DisplayPanel';
import { TerminalPanel } from '../components/panels/TerminalPanel';
import type { PanelTab } from '../store/session';

const TABS: { key: PanelTab; label: string; el: ReactElement }[] = [
  { key: 'code', label: 'Code', el: <CodePanel /> },
  { key: 'trace', label: 'Trace', el: <TracePanel /> },
  { key: 'display', label: 'Display', el: <DisplayPanel /> },
  { key: 'terminal', label: 'Terminal', el: <TerminalPanel /> },
];

export function MobileLayout() {
  const tab = useSession((s) => s.panelTab);
  const setTab = useSession((s) => s.setPanelTab);
  const active = TABS.find((t) => t.key === tab) ?? TABS[0];
  return (
    <div className="flex h-full flex-col" data-testid="mobile-layout">
      <nav className="flex border-b border-(--color-border) bg-(--color-bg)">
        {TABS.map((t) => (
          <button
            key={t.key}
            type="button"
            data-testid={`mobile-tab-${t.key}`}
            onClick={() => setTab(t.key)}
            className={`flex-1 px-2 py-2 text-sm ${
              tab === t.key
                ? 'text-(--color-accent) font-semibold'
                : 'text-(--color-fg-secondary)'
            }`}
          >
            {t.label}
          </button>
        ))}
      </nav>
      <div className="flex-1 overflow-hidden">{active.el}</div>
    </div>
  );
}

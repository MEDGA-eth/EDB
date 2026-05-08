import { Folder, Network, Eye, Terminal, Circle, Search } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { useSession, type ActivityKind } from '../store/session';

interface Item {
  key: ActivityKind;
  label: string;
  Icon: LucideIcon;
}

const ITEMS: Item[] = [
  { key: 'explorer', label: 'Explorer', Icon: Folder },
  { key: 'trace', label: 'Trace', Icon: Network },
  { key: 'variables', label: 'Variables', Icon: Eye },
  { key: 'terminal', label: 'Terminal', Icon: Terminal },
  { key: 'breakpoints', label: 'Breakpoints', Icon: Circle },
];

export function ActivityBar() {
  const active = useSession((s) => s.activeActivity);
  const setActivity = useSession((s) => s.setActivity);
  const setPaletteOpen = useSession((s) => s.setPaletteOpen);
  return (
    <nav
      className="flex w-12 flex-col border-r border-(--color-border) bg-(--color-bg)"
      data-testid="activity-bar"
      aria-label="Activity bar"
    >
      {ITEMS.map(({ key, label, Icon }) => {
        const isActive = key === active;
        return (
          <button
            key={key}
            type="button"
            onClick={() => setActivity(key)}
            title={label}
            aria-label={label}
            aria-pressed={isActive}
            data-testid={`activity-${key}`}
            className={`relative flex h-12 items-center justify-center text-(--color-fg-secondary) transition hover:text-(--color-fg) ${
              isActive ? 'text-(--color-fg)' : ''
            }`}
          >
            {isActive && (
              <span
                aria-hidden
                className="absolute top-0 bottom-0 left-0 w-0.5 bg-(--color-accent)"
              />
            )}
            <Icon size={20} />
          </button>
        );
      })}
      <div className="mt-auto flex flex-col">
        <button
          type="button"
          onClick={() => setPaletteOpen(true)}
          title="Open command palette (Ctrl+P)"
          aria-label="Open command palette"
          data-testid="activity-palette"
          className="flex h-12 items-center justify-center text-(--color-fg-secondary) transition hover:text-(--color-fg)"
        >
          <Search size={20} />
        </button>
      </div>
    </nav>
  );
}

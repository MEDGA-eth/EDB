import { Folder, Network, Eye, Search } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { useSession, type ActivityKind } from '../store/session';

interface Item {
  key: ActivityKind;
  label: string;
  Icon: LucideIcon | React.ComponentType<{ size?: number; 'aria-hidden'?: boolean }>;
  /** A CSS-variable expression for the icon's hue, distinct per-pane so the
   *  user can identify the panel by colour rather than reading the label. */
  tint: string;
}

/**
 * Custom breakpoint glyph — solid danger-coloured dot with a thin ring.
 * Used in place of lucide-react's plain `Circle` (which read as a generic
 * outline rather than the universal IDE breakpoint marker).
 */
function BreakpointGlyph({ size = 22 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" role="img" aria-hidden fill="none">
      <circle cx="12" cy="12" r="9" stroke="currentColor" strokeWidth="1.6" opacity="0.45" />
      <circle cx="12" cy="12" r="5" fill="var(--color-danger)" />
    </svg>
  );
}

const ITEMS: Item[] = [
  { key: 'explorer', label: 'Explorer', Icon: Folder, tint: 'var(--color-syn-type-std)' }, // sky
  { key: 'trace', label: 'Trace', Icon: Network, tint: 'var(--color-syn-func)' }, // violet
  { key: 'variables', label: 'Variables', Icon: Eye, tint: 'var(--color-syn-modifier)' }, // teal
  { key: 'breakpoints', label: 'Breakpoints', Icon: BreakpointGlyph, tint: 'var(--color-danger)' }, // red
];

export function ActivityBar() {
  const active = useSession((s) => s.activeActivity);
  const setActivity = useSession((s) => s.setActivity);
  const setPaletteOpen = useSession((s) => s.setPaletteOpen);
  return (
    <nav
      className="flex w-16 flex-col border-r border-(--color-border) bg-(--color-bg)"
      data-testid="activity-bar"
      aria-label="Activity bar"
    >
      {ITEMS.map(({ key, label, Icon, tint }) => {
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
            className={`relative flex h-16 flex-col items-center justify-center gap-1 transition ${
              isActive ? 'bg-(--color-accent)/10' : ''
            }`}
          >
            {isActive && (
              <span
                aria-hidden
                className="absolute top-0 bottom-0 left-0 w-[3px] bg-(--color-accent)"
              />
            )}
            <span style={{ color: tint }} aria-hidden>
              <Icon size={22} aria-hidden />
            </span>
            <span
              className={`text-[11px] font-medium leading-none ${
                isActive ? 'text-(--color-fg)' : 'text-(--color-fg-secondary)'
              }`}
            >
              {label}
            </span>
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
          className="flex h-16 flex-col items-center justify-center gap-1 text-(--color-fg-secondary) transition hover:text-(--color-fg)"
        >
          <Search size={22} aria-hidden />
          <span className="text-[11px] font-medium leading-none">Search</span>
        </button>
      </div>
    </nav>
  );
}

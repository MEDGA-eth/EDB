import type { LucideIcon } from 'lucide-react';
import type { ReactNode, ButtonHTMLAttributes } from 'react';

/** Shared toolbar wrapper — a thin row of action buttons. */
export function Toolbar({
  children,
  testid,
  className,
}: {
  children?: ReactNode;
  testid?: string;
  className?: string;
}) {
  return (
    <div
      role="toolbar"
      data-testid={testid}
      className={
        'flex h-9 shrink-0 items-center gap-2 border-b border-(--color-border) bg-(--color-bg) px-2 ' +
        (className ?? '')
      }
    >
      {children}
    </div>
  );
}

/** Vertical 1-px divider used between groups of buttons. */
export function ToolbarDivider() {
  return <span aria-hidden className="mx-1 h-5 w-px bg-(--color-border)" />;
}

/** Icon button with tooltip + active/disabled states. */
export interface ToolbarButtonProps
  extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'aria-label'> {
  icon: LucideIcon;
  label: string;
  active?: boolean;
  testid?: string;
}

export function ToolbarButton({
  icon: Icon,
  label,
  active = false,
  testid,
  className,
  ...rest
}: ToolbarButtonProps) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active}
      data-testid={testid}
      className={
        'flex h-7 w-7 items-center justify-center rounded-[var(--radius)] ' +
        'text-(--color-fg-secondary) transition-transform duration-75 ' +
        'hover:bg-(--color-bg-hover) hover:text-(--color-fg) ' +
        'active:scale-95 ' +
        'disabled:opacity-40 disabled:cursor-not-allowed ' +
        (active ? 'bg-(--color-bg-active) text-(--color-fg) ' : '') +
        (className ?? '')
      }
      {...rest}
    >
      <Icon size={16} aria-hidden />
    </button>
  );
}

/** A small inline label used as a section title inside a toolbar (optional). */
export function ToolbarLabel({ children }: { children: ReactNode }) {
  return (
    <span className="text-[11px] font-display font-semibold tracking-wide text-(--color-fg-tertiary) uppercase">
      {children}
    </span>
  );
}

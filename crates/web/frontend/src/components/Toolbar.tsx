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
        'flex h-8 shrink-0 items-center gap-2 border-b border-(--color-border) bg-(--color-bg) px-2 ' +
        (className ?? '')
      }
    >
      {children}
    </div>
  );
}

/** Vertical 1-px divider used between groups of buttons. */
export function ToolbarDivider() {
  return <span aria-hidden className="mx-1 h-4 w-px bg-(--color-border)" />;
}

/** A 24×24 icon button with tooltip + active/disabled states. */
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
        'flex h-6 w-6 items-center justify-center rounded-[var(--radius)] ' +
        'text-(--color-fg-secondary) transition ' +
        'hover:bg-(--color-bg-hover) hover:text-(--color-fg) ' +
        'disabled:opacity-40 disabled:cursor-not-allowed ' +
        (active ? 'bg-(--color-bg-active) text-(--color-fg) ' : '') +
        (className ?? '')
      }
      {...rest}
    >
      <Icon size={14} />
    </button>
  );
}

/** A small inline label used as a section title inside a toolbar (optional). */
export function ToolbarLabel({ children }: { children: ReactNode }) {
  return (
    <span className="text-[10px] font-display font-semibold tracking-wide text-(--color-fg-tertiary) uppercase">
      {children}
    </span>
  );
}

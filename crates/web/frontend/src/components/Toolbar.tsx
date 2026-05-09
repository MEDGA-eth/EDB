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
  /**
   * Render the label inline next to the icon (instead of only as a
   * tooltip). Keeps every action self-explanatory in dense toolbars
   * — set this on toolbars where users may not be familiar with the
   * iconography (Trace, Code editor, etc).
   */
  showLabel?: boolean;
}

export function ToolbarButton({
  icon: Icon,
  label,
  active = false,
  testid,
  showLabel = false,
  className,
  ...rest
}: ToolbarButtonProps) {
  if (showLabel) {
    return (
      <button
        type="button"
        title={label}
        aria-label={label}
        aria-pressed={active}
        data-testid={testid}
        className={
          'inline-flex h-7 items-center gap-1.5 rounded-[var(--radius)] px-2 ' +
          'text-[12px] text-(--color-fg-secondary) transition ' +
          'hover:bg-(--color-bg-hover) hover:text-(--color-fg) ' +
          'active:scale-95 ' +
          'disabled:opacity-40 disabled:cursor-not-allowed ' +
          (active ? 'bg-(--color-bg-active) text-(--color-fg) ' : '') +
          (className ?? '')
        }
        {...rest}
      >
        <Icon size={14} aria-hidden />
        <span>{label}</span>
      </button>
    );
  }
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

import { useState } from 'react';
import {
  Command,
  Eye,
  HelpCircle,
  Keyboard,
  LayoutGrid,
  MousePointerClick,
  Network,
  PlayCircle,
  Terminal,
  X,
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';

/**
 * Help overlay opened from the status bar's `?` button.
 *
 * Each section is rendered from a small data structure (sections + rows)
 * so the layout stays consistent and the markdown-blob version we used
 * to render doesn't drift typography out of sync with the rest of the UI.
 *
 * Sections are organised by *what the user is trying to do*, not by which
 * subsystem owns the binding. Stepping shortcuts live with terminal step
 * commands; both surface the same engine action.
 */
interface Row {
  /** Keyboard chord OR command literal — first column. */
  keys: string;
  /** Plain-language description — second column. */
  desc: string;
}

interface Section {
  title: string;
  Icon: LucideIcon;
  rows: Row[];
  /** A short paragraph rendered above the table. */
  intro?: string;
}

const SECTIONS: Section[] = [
  {
    title: 'Stepping',
    Icon: PlayCircle,
    intro:
      'Each Step variant moves the active snapshot. Step Into is the safest one — it advances exactly one snapshot. Step Over and Step Out trade precision for speed by skipping or unwinding call frames.',
    rows: [
      { keys: 'F5', desc: 'Continue — run to next breakpoint (or end of trace)' },
      { keys: '⇧F5', desc: 'Stop — park at the last snapshot' },
      { keys: '⌘⇧F5 / Ctrl+⇧F5', desc: 'Restart — jump back to snapshot 1' },
      { keys: '⌥F5', desc: 'Reverse Continue — run back to the previous breakpoint' },
      { keys: 'F11', desc: 'Step Into — advance one snapshot, descend into a CALL' },
      { keys: '⇧F11', desc: 'Step Out — run until the current frame returns' },
      { keys: 'F10', desc: 'Step Over (BETA) — same call depth; skip CALL bodies' },
      { keys: '⌥F10', desc: 'Reverse Step — inverse of Step Over' },
    ],
  },
  {
    title: 'Command palette',
    Icon: Command,
    intro:
      'The palette is the universal entry point — every command in the toolbar, sidebar, and terminal is reachable from here.',
    rows: [
      { keys: '⌘P / Ctrl+P', desc: 'Toggle the palette' },
      { keys: '⌘⇧P / Ctrl+⇧P', desc: 'Open the palette in command-mode (prefilled with `>`)' },
      { keys: 'Esc', desc: 'Close the palette' },
    ],
  },
  {
    title: 'Trace tree',
    Icon: Network,
    rows: [
      { keys: 'Click', desc: 'Reveal the source for the entry — does NOT change the snapshot' },
      { keys: 'Right-click', desc: 'Open menu: Jump to snapshot N · Reveal source · Expand/Collapse' },
      { keys: '⇧+Click', desc: 'Keyboard alternative for "Jump to snapshot"' },
    ],
  },
  {
    title: 'Editor (code view)',
    Icon: MousePointerClick,
    rows: [
      { keys: 'Click gutter dot', desc: 'Toggle a breakpoint on that line' },
      { keys: 'Cmd/Ctrl+F', desc: 'Find within the open file' },
      { keys: 'Toolbar: Reveal current', desc: 'Scroll to the line of the active snapshot' },
      { keys: 'Toolbar: Breakpoint here', desc: 'Add a breakpoint at the cursor line (uses selection start, so selecting a line works)' },
    ],
  },
  {
    title: 'Variables & Watch',
    Icon: Eye,
    intro:
      'The Variables sidebar shows decoded locals and state vars for the current snapshot. The Watch list re-evaluates each expression on every snapshot change.',
    rows: [
      { keys: 'Eye chip in terminal', desc: 'Save the last terminal expression as a watch' },
      { keys: 'Type expression in Watch', desc: 'Evaluated against every snapshot you step to' },
      { keys: 'Address value', desc: 'Click to open that contract\'s source in a new editor tab' },
    ],
  },
  {
    title: 'Terminal commands',
    Icon: Terminal,
    intro:
      'Anything that doesn\'t match a built-in keyword is treated as a Solidity expression and evaluated at the current snapshot.',
    rows: [
      { keys: 'continue / c', desc: 'Run to the next breakpoint' },
      { keys: 'step / s', desc: 'Step Into (single snapshot forward)' },
      { keys: 'over / o', desc: 'Step Over (skip CALL bodies)' },
      { keys: 'out', desc: 'Step Out (run until current frame returns)' },
      { keys: 'goto <n>', desc: 'Jump to snapshot N (1-based)' },
      { keys: 'break <addr>:<line>', desc: 'Add a source-line breakpoint' },
      { keys: 'break <addr>:pc=<pc>', desc: 'Add an opcode breakpoint' },
      { keys: 'bp', desc: 'List configured breakpoints' },
      { keys: 'unbreak <#>', desc: 'Remove a breakpoint by index' },
      { keys: 'clear', desc: 'Clear terminal history' },
      { keys: 'help', desc: 'Print this list' },
    ],
  },
  {
    title: 'Layout & panels',
    Icon: LayoutGrid,
    rows: [
      { keys: 'Drag tab', desc: 'Drop on an edge to split, drop on another tab to stack' },
      { keys: 'Drag splitter', desc: 'Resize the panel — sashes glow on hover' },
      { keys: 'Reopen pill', desc: 'Restore Display / Terminal if you closed them by mistake' },
      { keys: '×' , desc: 'Close a tab — recover from the Reopen pill on the toolbar' },
    ],
  },
];

export function HelpOverlay() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        data-testid="help-open"
        onClick={() => setOpen(true)}
        aria-label="Help"
        title="Help & shortcuts (?)"
        className="inline-flex items-center gap-1 rounded-[var(--radius)] px-2 py-0.5 text-(--color-fg-secondary) hover:bg-(--color-bg-hover) hover:text-(--color-fg)"
      >
        <HelpCircle size={14} aria-hidden />
        <span className="hidden lg:inline text-[12px]">Help</span>
      </button>
      {open && (
        <div
          role="dialog"
          aria-modal="true"
          aria-labelledby="help-title"
          data-testid="help-overlay"
          className="fixed inset-0 z-40 flex items-start justify-center overflow-y-auto bg-(--color-bg-root)/90 px-4 py-10"
          onClick={() => setOpen(false)}
        >
          <div
            data-testid="help-panel"
            className="w-full max-w-5xl rounded-lg border border-(--color-border-strong) bg-(--color-bg-elevated) shadow-[var(--shadow-lg)]"
            onClick={(e) => e.stopPropagation()}
          >
            <header className="flex items-center justify-between border-b border-(--color-border) px-6 py-4">
              <div className="flex items-center gap-3">
                <Keyboard size={22} className="text-(--color-accent)" aria-hidden />
                <div>
                  <h2 id="help-title" className="font-display text-[18px] font-bold text-(--color-fg)">
                    EDB &mdash; help &amp; shortcuts
                  </h2>
                  <p className="text-[12px] text-(--color-fg-tertiary)">
                    Source-level time-travel debugger. Everything you can do
                    is reachable from the Command Palette (
                    <kbd className="rounded border border-(--color-border) bg-(--color-bg) px-1 font-mono text-[11px]">⌘P</kbd>
                    ).
                  </p>
                </div>
              </div>
              <button
                type="button"
                onClick={() => setOpen(false)}
                aria-label="Close help"
                className="inline-flex h-8 w-8 items-center justify-center rounded-full text-(--color-fg-tertiary) hover:bg-(--color-bg-hover) hover:text-(--color-fg)"
              >
                <X size={18} aria-hidden />
              </button>
            </header>

            <div className="grid gap-6 p-6 md:grid-cols-2">
              {SECTIONS.map((s) => (
                <section
                  key={s.title}
                  className="rounded-md border border-(--color-border) bg-(--color-bg) p-4"
                >
                  <div className="mb-2 flex items-center gap-2">
                    <s.Icon size={16} className="text-(--color-accent)" aria-hidden />
                    <h3 className="font-display text-[14px] font-semibold tracking-wide text-(--color-fg) uppercase">
                      {s.title}
                    </h3>
                  </div>
                  {s.intro && (
                    <p className="mb-3 text-[12px] leading-relaxed text-(--color-fg-tertiary)">
                      {s.intro}
                    </p>
                  )}
                  <dl className="space-y-1.5">
                    {s.rows.map((r, i) => (
                      <div
                        key={i}
                        className="flex items-baseline gap-3 text-[13px]"
                      >
                        <dt className="shrink-0 font-mono text-[12px] text-(--color-fg)">
                          <kbd className="rounded border border-(--color-border) bg-(--color-bg-elevated) px-1.5 py-0.5 leading-none">
                            {r.keys}
                          </kbd>
                        </dt>
                        <dd className="text-(--color-fg-secondary)">{r.desc}</dd>
                      </div>
                    ))}
                  </dl>
                </section>
              ))}
            </div>

            <footer className="flex items-center justify-between gap-4 border-t border-(--color-border) bg-(--color-bg) px-6 py-3 text-[12px] text-(--color-fg-tertiary)">
              <span>
                Issues &amp; feedback:{' '}
                <a
                  href="https://github.com/edb-rs/edb/issues"
                  target="_blank"
                  rel="noreferrer"
                  className="text-(--color-accent) hover:underline"
                >
                  github.com/edb-rs/edb
                </a>
              </span>
              <button
                type="button"
                onClick={() => setOpen(false)}
                className="rounded-[var(--radius)] bg-(--color-accent) px-3 py-1 text-[12px] font-semibold text-white"
              >
                Close
              </button>
            </footer>
          </div>
        </div>
      )}
    </>
  );
}

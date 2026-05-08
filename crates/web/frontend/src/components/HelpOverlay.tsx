import { useState } from 'react';
import ReactMarkdown from 'react-markdown';

const HELP_MD = `
### Navigation
- **n / Next snapshot** — go forward one step
- **p / Prev snapshot** — go back one step
- **N / next call** — jump to next message-call
- **P / prev call** — jump to previous message-call

### Terminal
- \`expr\` — evaluate a Solidity expression at the current snapshot
- \`break <addr>:<line>\` — set a source-line breakpoint
- \`break <addr>:pc=<pc>\` — set an opcode breakpoint
- \`bp\` — list breakpoints
- \`unbreak <n>\` — remove breakpoint #n
`;

export function HelpOverlay() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        data-testid="help-open"
        onClick={() => setOpen(true)}
        className="rounded-[var(--radius)] px-3 py-1 hover:bg-(--color-bg-hover)"
      >
        ?
      </button>
      {open && (
        <div
          role="dialog"
          aria-modal="true"
          data-testid="help-overlay"
          className="fixed inset-0 z-40 flex items-center justify-center bg-(--color-bg-root)/85"
          onClick={() => setOpen(false)}
        >
          <div
            data-testid="help-panel"
            className="max-h-[80vh] w-[640px] overflow-y-auto rounded-[var(--radius)] bg-(--color-bg-elevated) p-6 shadow-[var(--shadow-lg)]"
            onClick={(e) => e.stopPropagation()}
          >
            <ReactMarkdown>{HELP_MD}</ReactMarkdown>
            <button
              type="button"
              onClick={() => setOpen(false)}
              className="mt-4 rounded-[var(--radius)] bg-(--color-accent) px-3 py-1 text-white"
            >
              Close
            </button>
          </div>
        </div>
      )}
    </>
  );
}

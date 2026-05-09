import { useState } from 'react';
import ReactMarkdown from 'react-markdown';

const HELP_MD = `
### Debugger shortcuts (VSCode-style)
- **F5** — continue
- **Shift+F5** — stop (run to end)
- **Cmd/Ctrl+Shift+F5** — restart (jump to first snapshot)
- **Alt+F5** — reverse continue
- **F10** — step over
- **Alt+F10** — reverse step over
- **F11** — step into (next snapshot)
- **Shift+F11** — step out
- **Cmd/Ctrl+P** — toggle command palette
- **Cmd/Ctrl+Shift+P** — palette in command-mode

### Terminal commands
- \`<expr>\` — evaluate a Solidity expression at the current snapshot
- \`continue\` / \`c\` — run to next breakpoint
- \`step\` / \`s\` — step into (next snapshot)
- \`next\` / \`n\` — same as step (single snapshot forward)
- \`over\` / \`o\` — step over
- \`out\` — step out
- \`goto <n>\` — jump to snapshot N
- \`break <addr>:<line>\` — set a source-line breakpoint
- \`break <addr>:pc=<pc>\` — set an opcode breakpoint
- \`bp\` — list breakpoints
- \`unbreak <n>\` — remove breakpoint #n
- \`clear\` — clear terminal history
- \`help\` — show this help
`;

export function HelpOverlay() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        data-testid="help-open"
        onClick={() => setOpen(true)}
        aria-label="Help"
        title="Help (?)"
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

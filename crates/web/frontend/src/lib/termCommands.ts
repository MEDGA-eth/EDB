import type { QueryClient } from '@tanstack/react-query';
import { getCommand, type CommandCtx } from './commands';
import type { Breakpoint } from './types';
import { useSession } from '../store/session';

/**
 * Run a built-in terminal command (TUI parity: `step`, `goto`, `bp`, …).
 *
 * Returns:
 *   - `{ handled: true, message?: string }` — recognized command. Caller
 *     emits `message` (when present) via `appendTerminal({kind:'message'})`.
 *   - `{ handled: false }` — not a command; caller should fall through to
 *     expression evaluation (the original terminal semantics).
 *
 * Why a separate module: keeps TerminalPanel.tsx focused on rendering and
 * lets us unit-test the dispatch table without spinning up React.
 */
export interface TermCommandCtx {
  queryClient: QueryClient;
  snapshotCount: number;
  /** invoked when the command resolves to "show this help dialog" */
  openHelp?: () => void;
}

export interface TermCommandResult {
  handled: boolean;
  message?: string;
}

const ALIASES: Record<string, string> = {
  s: 'step',
  n: 'next',
  o: 'over',
  c: 'continue',
  rc: 'reverse-continue',
  '?': 'help',
};

const NAV_MAP: Record<string, string> = {
  step: 'nav.next',
  next: 'nav.next',
  into: 'nav.next',
  over: 'nav.step-over',
  out: 'nav.step-out',
  continue: 'nav.continue',
  'reverse-continue': 'nav.reverse-continue',
};

export function runTermCommand(input: string, ctx: TermCommandCtx): TermCommandResult {
  const trimmed = input.trim();
  if (!trimmed) return { handled: false };

  // Split on the first run of whitespace; everything after is the arg tail.
  const m = /^(\S+)(?:\s+(.*))?$/.exec(trimmed);
  if (!m) return { handled: false };
  const verbRaw = m[1].toLowerCase();
  const tail = m[2]?.trim() ?? '';
  const verb = ALIASES[verbRaw] ?? verbRaw;

  // ---- navigation passthroughs (step / next / over / out / continue …) ----
  if (verb in NAV_MAP) {
    const cmd = getCommand(NAV_MAP[verb]);
    if (!cmd) return { handled: true, message: `internal: command ${NAV_MAP[verb]} missing` };
    const cmdCtx: CommandCtx = { queryClient: ctx.queryClient, snapshotCount: ctx.snapshotCount };
    if (cmd.enabled && !cmd.enabled(cmdCtx)) {
      return { handled: true, message: `\`${verb}\` not available at this snapshot.` };
    }
    cmd.run(cmdCtx);
    return { handled: true };
  }

  // ---- goto <n> ----
  if (verb === 'goto' || verb === 'g') {
    const n = parseInt(tail, 10);
    if (!Number.isFinite(n) || n < 0) {
      return { handled: true, message: 'usage: `goto <snapshot-id>`' };
    }
    const max = Math.max(0, ctx.snapshotCount - 1);
    const target = Math.min(n, max);
    useSession.getState().setSnapshotId(target);
    return { handled: true, message: `→ snapshot ${target}${target !== n ? ` (clamped from ${n})` : ''}` };
  }

  // ---- bp / break / unbreak ----
  if (verb === 'bp' || verb === 'breakpoints') {
    const bps = useSession.getState().breakpoints;
    if (bps.length === 0) return { handled: true, message: '_no breakpoints set_' };
    const lines = bps.map((bp, i) => `**#${i}** ${describeBreakpoint(bp)}${bp.condition ? ` _if_ \`${bp.condition}\`` : ''}${bp.enabled === false ? ' _(disabled)_' : ''}`);
    return { handled: true, message: lines.join('\n\n') };
  }

  if (verb === 'break' || verb === 'b') {
    const parsed = parseBreakSpec(tail);
    if (!parsed) {
      return {
        handled: true,
        message: 'usage: `break <addr>:<line>` or `break <addr>:pc=<pc>`',
      };
    }
    useSession.getState().addBreakpoint({
      loc: parsed,
      condition: null,
      enabled: true,
    });
    const idx = useSession.getState().breakpoints.length - 1;
    return { handled: true, message: `breakpoint **#${idx}** added at ${describeBreakpoint({ loc: parsed })}` };
  }

  if (verb === 'unbreak' || verb === 'unbp') {
    const n = parseInt(tail, 10);
    if (!Number.isFinite(n) || n < 0) return { handled: true, message: 'usage: `unbreak <#>`' };
    const bps = useSession.getState().breakpoints;
    if (n >= bps.length) return { handled: true, message: `no breakpoint #${n}` };
    useSession.getState().removeBreakpoint(n);
    return { handled: true, message: `breakpoint **#${n}** removed` };
  }

  // ---- clear / help ----
  if (verb === 'clear' || verb === 'cls') {
    useSession.getState().clearTerminal();
    return { handled: true };
  }

  if (verb === 'help') {
    if (ctx.openHelp) ctx.openHelp();
    return { handled: true, message: HELP_TEXT };
  }

  return { handled: false };
}

function parseBreakSpec(tail: string): Breakpoint['loc'] | null {
  // `<addr>:pc=<pc>` opcode breakpoint
  const op = /^(0x[0-9a-fA-F]{40})\s*:\s*pc\s*=\s*(\d+)$/.exec(tail);
  if (op) {
    return { kind: 'Opcode', bytecode_address: op[1].toLowerCase(), pc: parseInt(op[2], 10) };
  }
  // `<addr>:<line>` source-line breakpoint. file_path defaults to '' — the
  // engine resolves the active file for that address. The user can edit
  // this through the Breakpoints sidebar if they need a specific path.
  const src = /^(0x[0-9a-fA-F]{40})\s*:\s*(\d+)$/.exec(tail);
  if (src) {
    return {
      kind: 'Source',
      bytecode_address: src[1].toLowerCase(),
      file_path: '',
      line_number: parseInt(src[2], 10),
    };
  }
  return null;
}

function describeBreakpoint(bp: Pick<Breakpoint, 'loc'>): string {
  if (!bp.loc) return '_(condition-only)_';
  if (bp.loc.kind === 'Opcode') return `\`${bp.loc.bytecode_address}\` pc=\`${bp.loc.pc}\``;
  return `\`${bp.loc.bytecode_address}\`${bp.loc.file_path ? ` _${bp.loc.file_path}_` : ''}:**${bp.loc.line_number}**`;
}

const HELP_TEXT = `**Built-in commands**

- \`step\` / \`s\` / \`next\` / \`n\` — single snapshot forward
- \`over\` / \`o\` — step over (call boundary)
- \`out\` — step out
- \`continue\` / \`c\` — run to next breakpoint
- \`reverse-continue\` / \`rc\` — run back to previous breakpoint
- \`goto <n>\` — jump to snapshot N
- \`break <addr>:<line>\` — source-line breakpoint
- \`break <addr>:pc=<pc>\` — opcode breakpoint
- \`bp\` — list breakpoints
- \`unbreak <#>\` — remove breakpoint
- \`clear\` — clear terminal
- \`help\` — this list

Anything else is treated as a Solidity expression and evaluated at the current snapshot.`;

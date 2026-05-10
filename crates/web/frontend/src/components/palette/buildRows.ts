import { COMMANDS, type Command, type CommandCtx, type CommandGroup } from '../../lib/commands';
import { rankBy } from '../../lib/fuzzyMatch';
import { PALETTE_RESULT_CAP } from '../../lib/constants';
import type { AvailableFile } from '../../hooks/useAvailableFiles';

export type PaletteRow =
  | { kind: 'command'; id: string; label: string; hint?: string; group?: CommandGroup; run(): void }
  | { kind: 'file'; id: string; label: string; hint?: string; run(): void }
  | { kind: 'snapshot'; id: string; label: string; hint?: string; run(): void }
  | { kind: 'address'; id: string; label: string; hint?: string; run(): void };

function shortAddr(a: string): string {
  return `${a.slice(0, 6)}…${a.slice(-4)}`;
}

function commandToRow(c: Command, ctx: CommandCtx): PaletteRow {
  return {
    kind: 'command',
    id: `cmd:${c.id}`,
    label: c.label,
    hint: c.hint ? c.hint : c.group,
    group: c.group,
    run: () => c.run(ctx),
  };
}

export interface BuildRowsCtx {
  ctx: CommandCtx;
  snapshotCount: number;
  files: AvailableFile[];
  addresses: string[];
  openFile(args: { addr: string; path: string }): void;
  setSnap(id: number): void;
  loadRecent(): string[];
}

/**
 * Compute the filtered + ranked palette row list for the given query and
 * surrounding context. Pure function (modulo the `loadRecent` callback) so
 * it can be unit-tested without a React tree.
 */
export function buildRows(query: string, b: BuildRowsCtx): PaletteRow[] {
  const trimmed = query.trim();
  const isCommandMode = trimmed.startsWith('>');
  const isSnapshotMode = trimmed.startsWith(':') || trimmed.startsWith('#');
  const stripped = isCommandMode || isSnapshotMode ? trimmed.slice(1).trim() : trimmed;

  /* commands */
  const enabledCommands = COMMANDS.filter((c) => (c.enabled ? c.enabled(b.ctx) : true));
  const commandRows: PaletteRow[] = enabledCommands.map((c) => commandToRow(c, b.ctx));

  /* snapshots, only generate at most 50 to avoid huge lists */
  const snapshotRows: PaletteRow[] = [];
  if (b.snapshotCount > 0) {
    const limit = Math.min(b.snapshotCount, 50);
    for (let i = 0; i < limit; i += 1) {
      snapshotRows.push({
        kind: 'snapshot',
        id: `snap:${i}`,
        label: `Go to snapshot ${i}`,
        hint: `${i} / ${b.snapshotCount}`,
        run: () => b.setSnap(i),
      });
    }
  }

  /* files */
  const fileRows: PaletteRow[] = b.files.map((f) => ({
    kind: 'file',
    id: `file:${f.addr}::${f.path}`,
    label:
      f.path === '<disasm>'
        ? `<disasm> @ ${shortAddr(f.addr)}`
        : (f.path.split('/').pop() ?? f.path),
    hint: `${shortAddr(f.addr)} · ${f.path}`,
    run: () => b.openFile({ addr: f.addr, path: f.path }),
  }));

  /* addresses */
  const addressRows: PaletteRow[] = b.addresses.map((addr) => ({
    kind: 'address',
    id: `addr:${addr}`,
    label: shortAddr(addr),
    hint: addr,
    run: () => {
      // jumping to an address doesn't change snapshot, we open its disasm/source as fallback
      const file = b.files.find((f) => f.addr === addr);
      if (file) b.openFile({ addr: file.addr, path: file.path });
    },
  }));

  /* prefix routing */
  let pool: PaletteRow[];
  if (isCommandMode) pool = commandRows;
  else if (isSnapshotMode) pool = snapshotRows;
  else pool = [...fileRows, ...addressRows, ...commandRows];

  /* empty input → recent first */
  if (!stripped) {
    const recent = new Set(b.loadRecent());
    const recentRows = pool.filter((r) => recent.has(r.id));
    const rest = pool.filter((r) => !recent.has(r.id));
    return [...recentRows, ...rest].slice(0, PALETTE_RESULT_CAP);
  }

  return rankBy(pool, stripped, (r) => `${r.label} ${r.hint ?? ''}`)
    .slice(0, PALETTE_RESULT_CAP)
    .map((r) => r.item);
}

import { describe, expect, test } from 'bun:test';
import { QueryClient } from '@tanstack/react-query';
import { buildRows, type BuildRowsCtx } from './buildRows';
import type { CommandCtx } from '../../lib/commands';

const ADDR_A = '0x' + 'a'.repeat(40);

function makeCtx(over: Partial<BuildRowsCtx> = {}): BuildRowsCtx {
  const cmdCtx: CommandCtx = { queryClient: new QueryClient(), snapshotCount: 4 };
  return {
    ctx: cmdCtx,
    snapshotCount: 4,
    files: [{ addr: ADDR_A, path: 'X.sol' }],
    addresses: [ADDR_A],
    openFile: () => {},
    setSnap: () => {},
    loadRecent: () => [],
    ...over,
  };
}

describe('buildRows', () => {
  test('command-mode (>) returns only commands', () => {
    const rows = buildRows('>theme', makeCtx());
    expect(rows.every((r) => r.kind === 'command')).toBe(true);
    expect(rows.find((r) => r.id === 'cmd:view.toggle-theme')).toBeTruthy();
  });

  test('snapshot-mode (:) returns only snapshot rows', () => {
    const rows = buildRows(':2', makeCtx({ snapshotCount: 5 }));
    expect(rows.every((r) => r.kind === 'snapshot')).toBe(true);
    expect(rows.find((r) => r.id === 'snap:2')).toBeTruthy();
  });

  test('hash-mode (#) also routes to snapshots', () => {
    const rows = buildRows('#1', makeCtx({ snapshotCount: 5 }));
    expect(rows.every((r) => r.kind === 'snapshot')).toBe(true);
  });

  test('empty query returns recent rows first then the rest', () => {
    const rows = buildRows('', makeCtx({ loadRecent: () => ['cmd:view.toggle-theme'] }));
    expect(rows[0]?.id).toBe('cmd:view.toggle-theme');
  });

  test('non-prefix query mixes files, addresses, and commands', () => {
    const rows = buildRows('X', makeCtx());
    // X.sol should rank somewhere in the result.
    expect(rows.some((r) => r.id === `file:${ADDR_A}::X.sol`)).toBe(true);
  });

  test('disasm files render with angle brackets in label', () => {
    const rows = buildRows('', makeCtx({ files: [{ addr: ADDR_A, path: '<disasm>' }] }));
    const fileRow = rows.find((r) => r.kind === 'file');
    expect(fileRow?.label).toContain('<disasm>');
  });
});

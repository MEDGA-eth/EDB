import { describe, expect, test } from 'bun:test';
import { tokenize } from './opcodeTokens';

describe('tokenize', () => {
  test('classifies opcode, hex, address, comment', () => {
    const t = tokenize('PUSH20 0x' + '0'.repeat(40) + ' ; pushes target');
    expect(t.find((x) => x.kind === 'op')?.text).toBe('PUSH20');
    expect(t.find((x) => x.kind === 'addr')).toBeTruthy();
    expect(t.find((x) => x.kind === 'comment')?.text.startsWith(';')).toBe(true);
  });

  test('classifies hex constants', () => {
    const t = tokenize('PUSH1 0x60');
    expect(t.find((x) => x.kind === 'num')?.text).toBe('0x60');
  });
});

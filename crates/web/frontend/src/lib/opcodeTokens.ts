export type OpcodeToken = { kind: 'op' | 'num' | 'addr' | 'comment' | 'text'; text: string };

const ADDR_RE = /^0x[0-9a-fA-F]{40}$/;
const HEX_RE = /^0x[0-9a-fA-F]+$/;
const OPCODE_RE = /^[A-Z][A-Z0-9_]+$/;

export function tokenize(line: string): OpcodeToken[] {
  const out: OpcodeToken[] = [];
  // Strip and handle inline ';' comment first
  const semi = line.indexOf(';');
  const code = semi >= 0 ? line.slice(0, semi) : line;
  const comment = semi >= 0 ? line.slice(semi) : null;

  for (const part of code.split(/(\s+)/)) {
    if (!part) continue;
    if (/^\s+$/.test(part)) {
      out.push({ kind: 'text', text: part });
      continue;
    }
    if (ADDR_RE.test(part)) out.push({ kind: 'addr', text: part });
    else if (HEX_RE.test(part) || /^\d+$/.test(part)) out.push({ kind: 'num', text: part });
    else if (OPCODE_RE.test(part)) out.push({ kind: 'op', text: part });
    else out.push({ kind: 'text', text: part });
  }
  if (comment) out.push({ kind: 'comment', text: comment });
  return out;
}

/**
 * Find the line index in a disasm string corresponding to a given PC.
 * The disasm format is `${pc.toString(16).padStart(4,'0')}: <opcode>` per
 * line (see `disasmFromCodes` in lib/types.ts), so we just match the
 * leading 4-hex prefix. Returns -1 when the PC isn't present (e.g. PC
 * mid-PUSH-data, or a snapshot whose code we don't have yet).
 */
export function pcLineIndex(disasm: string, pc: number): number {
  if (!Number.isFinite(pc) || pc < 0) return -1;
  const prefix = `${pc.toString(16).padStart(4, '0')}:`;
  const lines = disasm.split('\n');
  for (let i = 0; i < lines.length; i += 1) {
    if (lines[i].startsWith(prefix)) return i;
  }
  return -1;
}

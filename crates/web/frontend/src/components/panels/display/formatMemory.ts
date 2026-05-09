/** Format a memory blob as 32-byte rows of hex with leading offsets. */
export function formatMemory(mem: number[]): string {
  const rows: string[] = [];
  for (let i = 0; i < mem.length; i += 32) {
    const slice = mem
      .slice(i, i + 32)
      .map((b) => b.toString(16).padStart(2, '0'))
      .join('');
    rows.push(`${i.toString(16).padStart(6, '0')}: ${slice}`);
  }
  return rows.join('\n');
}

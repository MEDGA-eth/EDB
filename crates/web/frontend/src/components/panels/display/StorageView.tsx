import { useStorageDiff } from '../../../hooks/useStorageDiff';

export function StorageView({ id }: { id: number }) {
  const { data, error } = useStorageDiff(id);
  if (error) return <span>{(error as Error).message}</span>;
  if (!data) return <span>Loading…</span>;
  // Drop slots that didn't change — the engine sometimes echoes touched-but-
  // unchanged slots, and rendering them with red strikethrough → green text
  // visually implies a mutation that didn't happen.
  const rows = data.filter((d) => d.before !== d.after);
  if (rows.length === 0)
    return <span className="text-(--color-fg-tertiary)">(no storage changes)</span>;
  return (
    <table className="w-full">
      <tbody>
        {rows.map((d, i) => (
          <tr key={i} data-testid={`storage-row-${i}`}>
            <td className="pr-3 text-(--color-fg-secondary)">{d.slot}</td>
            <td className="pr-3 line-through text-(--color-danger)">{d.before ?? '∅'}</td>
            <td className="text-(--color-success)">{d.after ?? '∅'}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

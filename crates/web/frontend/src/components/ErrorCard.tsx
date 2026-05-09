import { SchemaError } from '../lib/rpc';

interface Props {
  code?: number;
  message: string;
  onRetry?: () => void;
  /**
   * Optional underlying error. When this is a {@link SchemaError}, the card
   * surfaces the failing field path + expected vs received in a compact
   * details block, so a "schema mismatch in foo" message becomes actionable.
   */
  cause?: unknown;
}

interface SchemaIssueLine {
  path: string;
  expected: string;
  received: string;
}

function firstSchemaIssue(err: SchemaError): SchemaIssueLine | null {
  const issue = err.issues.errors?.[0] ?? err.issues.issues?.[0];
  if (!issue) return null;
  const path =
    Array.isArray(issue.path) && issue.path.length > 0
      ? issue.path.map((p) => (typeof p === 'number' ? `[${p}]` : `.${p}`)).join('').replace(/^\./, '')
      : '<root>';
  // zod issues carry `expected` and `received` on the invalid_type kind;
  // for other kinds we fall back to the issue.code so we always show something.
  const inv = issue as { expected?: string; received?: string; code?: string };
  const expected = inv.expected ?? inv.code ?? 'valid';
  const received = inv.received ?? 'invalid';
  return { path, expected, received };
}

export function ErrorCard({ code, message, onRetry, cause }: Props) {
  const schemaCause = cause instanceof SchemaError ? cause : undefined;
  const issue = schemaCause ? firstSchemaIssue(schemaCause) : null;
  return (
    <div
      role="alert"
      data-testid="error-card"
      className="rounded-[var(--radius)] border border-(--color-danger) bg-(--color-bg-elevated) p-4 text-(--color-fg)"
    >
      <div className="font-display font-semibold text-(--color-danger)">
        Error{code !== undefined ? ` (${code})` : ''}
      </div>
      <div className="mt-1 text-sm">{message}</div>
      {schemaCause && issue && (
        <pre
          data-testid="error-card-schema-detail"
          className="mt-2 whitespace-pre-wrap rounded bg-(--color-bg-hover) p-2 font-mono text-[12px] text-(--color-fg-secondary)"
        >
          {`schema mismatch in ${schemaCause.method}\n  at ${issue.path}: expected ${issue.expected}, received ${issue.received}`}
        </pre>
      )}
      {onRetry && (
        <button
          type="button"
          onClick={onRetry}
          className="mt-2 rounded-[var(--radius)] bg-(--color-accent) px-3 py-1 text-white hover:bg-(--color-accent)/90"
        >
          Retry
        </button>
      )}
    </div>
  );
}

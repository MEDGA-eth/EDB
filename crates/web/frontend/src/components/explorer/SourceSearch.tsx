import { useEffect, useState, type ReactNode } from 'react';
import { FileCode2, Regex, Search, X } from 'lucide-react';
import { useSearchSources, MIN_SEARCH_LEN } from '../../hooks/useSearchSources';
import { useSession } from '../../store/session';
import { ErrorBoundary } from '../ErrorBoundary';

function basename(p: string): string {
  const idx = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
  return idx === -1 ? p : p.slice(idx + 1);
}
function dirname(p: string): string {
  const idx = p.lastIndexOf('/');
  return idx === -1 ? '' : p.slice(0, idx);
}

/**
 * Global source-text search. Renders a sticky search box at the top of the
 * explorer pane. While a query is active it replaces `children` (the file
 * tree) with grouped match results; clicking a result opens the file and
 * scrolls to the matching line.
 */
export function SourceSearch({ children }: { children?: ReactNode }) {
  return (
    <ErrorBoundary label="SourceSearch">
      <SourceSearchInner>{children}</SourceSearchInner>
    </ErrorBoundary>
  );
}

function SourceSearchInner({ children }: { children?: ReactNode }) {
  const [input, setInput] = useState('');
  const [debounced, setDebounced] = useState('');
  useEffect(() => {
    const t = setTimeout(() => setDebounced(input), 250);
    return () => clearTimeout(t);
  }, [input]);

  const useRegex = useSession((s) => s.searchUseRegex);
  const toggleRegex = useSession((s) => s.toggleSearchRegex);

  const trimmed = debounced.trim();
  const active = trimmed.length >= MIN_SEARCH_LEN;
  const { data, isFetching, isError, error } = useSearchSources(debounced, useRegex);
  const openFileAtLine = useSession((s) => s.openFileAtLine);

  return (
    <div className="font-display">
      <div className="sticky top-0 z-10 flex items-center gap-1.5 border-b border-(--color-border) bg-(--color-bg) px-2 py-1.5">
        <Search size={12} className="shrink-0 text-(--color-fg-tertiary)" />
        <input
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder={useRegex ? 'Search by regex…' : 'Search all sources…'}
          spellCheck={false}
          autoComplete="off"
          data-testid="source-search-input"
          className="min-w-0 flex-1 bg-transparent text-xs text-(--color-fg) outline-none placeholder:text-(--color-fg-tertiary)"
        />
        {input && (
          <button
            type="button"
            onClick={() => setInput('')}
            aria-label="Clear search"
            data-testid="source-search-clear"
            className="shrink-0 rounded p-0.5 text-(--color-fg-tertiary) hover:bg-(--color-bg-hover)"
          >
            <X size={12} />
          </button>
        )}
        <button
          type="button"
          onClick={toggleRegex}
          aria-label="Use regular expression"
          aria-pressed={useRegex}
          title="Use regular expression"
          data-testid="source-search-regex-toggle"
          className={`shrink-0 rounded p-0.5 ${
            useRegex
              ? 'bg-(--color-accent-dim) text-(--color-accent)'
              : 'text-(--color-fg-tertiary) hover:bg-(--color-bg-hover)'
          }`}
        >
          <Regex size={13} />
        </button>
      </div>

      {!active ? (
        children
      ) : (
        <div data-testid="source-search-results">
          {isFetching && (
            <div className="px-3 py-2 text-xs text-(--color-fg-tertiary)">Searching…</div>
          )}
          {isError && (
            <div className="px-3 py-2 text-xs text-(--color-danger)">
              Search failed: {(error as Error).message}
            </div>
          )}
          {!isFetching && data && data.error && (
            <div className="px-3 py-2 text-xs text-(--color-danger)" data-testid="source-search-invalid-regex">
              Invalid regex: {data.error}
            </div>
          )}
          {!isFetching && data && !data.error && data.files.length === 0 && (
            <div className="px-3 py-2 text-xs text-(--color-fg-tertiary)">
              No matches for “{trimmed}”.
            </div>
          )}
          {data &&
            data.files.map((f) => (
              <div key={f.path} className="select-none">
                <div
                  className="flex items-center gap-1.5 px-2 py-1"
                  title={f.path}
                  data-testid={`source-search-file-${f.path}`}
                >
                  <FileCode2 size={12} className="shrink-0 text-(--color-syn-type-std)" />
                  <span className="shrink-0 truncate font-mono text-xs font-semibold text-(--color-fg-secondary)">
                    {basename(f.path)}
                  </span>
                  <span className="min-w-0 truncate font-mono text-[10.5px] text-(--color-fg-tertiary)">
                    {dirname(f.path)}
                  </span>
                  <span className="ml-auto shrink-0 rounded-full bg-(--color-bg-hover) px-1.5 text-[10px] text-(--color-fg-tertiary)">
                    {f.matches.length}
                  </span>
                </div>
                {f.matches.map((m) => {
                  const addr = f.addresses[0];
                  return (
                    <button
                      key={m.line}
                      type="button"
                      disabled={!addr}
                      onClick={() => addr && openFileAtLine({ addr, path: f.path, line: m.line })}
                      data-testid="source-search-hit"
                      data-path={f.path}
                      data-line={m.line}
                      className="flex w-full items-baseline gap-2 py-0.5 pr-2 pl-6 text-left outline-none hover:bg-(--color-bg-hover) focus:bg-(--color-bg-active)"
                    >
                      <span className="shrink-0 text-right font-mono text-[10.5px] tabular-nums text-(--color-fg-tertiary)">
                        {m.line}
                      </span>
                      <span className="truncate font-mono text-[11.5px] text-(--color-fg)">
                        {m.text}
                      </span>
                    </button>
                  );
                })}
              </div>
            ))}
          {data?.truncated && (
            <div className="px-3 py-2 text-[11px] text-(--color-fg-tertiary)">
              Showing the first {data.total_matches} matches; refine your query to narrow results.
            </div>
          )}
        </div>
      )}
    </div>
  );
}

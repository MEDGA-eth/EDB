import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { SourceSearch } from './SourceSearch';
import { makeWrapper, mockRpc } from '../../hooks/_test-utils';
import { useSession } from '../../store/session';

const ADDR = '0x' + 'a'.repeat(40);

describe('<SourceSearch />', () => {
  afterEach(() => {
    cleanup();
    useSession.setState({ openFiles: [], activeFileId: null, revealRequest: null });
  });

  test('shows the tree (children) until a query is entered', async () => {
    mockRpc({});
    const { wrapper } = makeWrapper();
    render(
      <SourceSearch>
        <div data-testid="tree-placeholder">tree</div>
      </SourceSearch>,
      { wrapper },
    );
    expect(screen.getByTestId('tree-placeholder')).toBeTruthy();
    expect(screen.queryByTestId('source-search-results')).toBeNull();
  });

  test('searches, lists grouped matches, and jumps to a clicked line', async () => {
    mockRpc({
      edb_searchSources: (params) => {
        const query = (params as string[])[0];
        expect(query).toBe('transfer');
        return {
          query,
          truncated: false,
          total_matches: 2,
          files: [
            {
              path: 'src/Token.sol',
              addresses: [ADDR],
              matches: [
                { line: 12, text: 'function transfer(address to) {' },
                { line: 30, text: '// transfer hook' },
              ],
            },
          ],
        };
      },
    });
    const { wrapper } = makeWrapper();
    render(
      <SourceSearch>
        <div data-testid="tree-placeholder">tree</div>
      </SourceSearch>,
      { wrapper },
    );

    await userEvent.type(screen.getByTestId('source-search-input'), 'transfer');

    // Debounced query (250ms) resolves and results replace the tree.
    await waitFor(() => expect(screen.getByTestId('source-search-results')).toBeTruthy());
    expect(screen.queryByTestId('tree-placeholder')).toBeNull();

    const hits = await waitFor(() => {
      const found = screen.getAllByTestId('source-search-hit');
      expect(found.length).toBe(2);
      return found;
    });

    // Clicking the second hit opens the file and requests a scroll to line 30.
    await userEvent.click(hits[1]!);
    await waitFor(() => {
      const s = useSession.getState();
      expect(s.openFiles.some((f) => f.path === 'src/Token.sol' && f.addr === ADDR)).toBe(true);
      expect(s.revealRequest?.line).toBe(30);
      expect(s.revealRequest?.fileId).toBe(`${ADDR}::src/Token.sol`);
    });
  });

  test('renders an empty-state when there are no matches', async () => {
    mockRpc({
      edb_searchSources: (params) => ({
        query: (params as string[])[0],
        truncated: false,
        total_matches: 0,
        files: [],
      }),
    });
    const { wrapper } = makeWrapper();
    render(<SourceSearch />, { wrapper });
    await userEvent.type(screen.getByTestId('source-search-input'), 'zzz');
    await waitFor(() => expect(screen.getByText(/No matches for/)).toBeTruthy());
  });
});

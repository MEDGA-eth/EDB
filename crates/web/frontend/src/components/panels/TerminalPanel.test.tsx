import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { TerminalPanel } from './TerminalPanel';
import { makeWrapper, mockRpc } from '../../hooks/_test-utils';
import { useSession } from '../../store/session';

describe('<TerminalPanel />', () => {
  beforeEach(() => useSession.getState().clearTerminal());
  afterEach(cleanup);

  test('appends input + result on submit', async () => {
    mockRpc({ edb_evalOnSnapshot: (params) => ({ value: 42, params }) });
    const { wrapper } = makeWrapper();
    render(<TerminalPanel />, { wrapper });
    const input = screen.getByTestId('terminal-input');
    await userEvent.type(input, 'block.number{enter}');
    await waitFor(() => expect(screen.getByTestId('term-result')).toBeInTheDocument());
    expect(screen.getByTestId('term-input').textContent).toContain('block.number');
  });

  test('shows error line on RPC error', async () => {
    mockRpc({ edb_evalOnSnapshot: () => { const e: Error & { code?: number } = new Error('boom'); e.code = -33006; throw e; } });
    const { wrapper } = makeWrapper();
    render(<TerminalPanel />, { wrapper });
    await userEvent.type(screen.getByTestId('terminal-input'), 'x{enter}');
    await waitFor(() => expect(screen.getByTestId('term-error')).toBeInTheDocument());
  });

  test('ignores empty submit', async () => {
    const { wrapper } = makeWrapper();
    render(<TerminalPanel />, { wrapper });
    await userEvent.type(screen.getByTestId('terminal-input'), '   {enter}');
    expect(useSession.getState().terminalHistory).toHaveLength(0);
  });
});

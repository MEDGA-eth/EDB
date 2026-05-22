import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { TerminalPanel, computeTermSuggestions } from './TerminalPanel';
import { makeWrapper, mockRpc } from '../../hooks/_test-utils';
import { useSession } from '../../store/session';

const UINT_ONE = { Ok: { type: 'Uint', value: { bits: 256, value: '1' } } };

describe('<TerminalPanel />', () => {
  beforeEach(() => useSession.getState().clearTerminal());
  afterEach(cleanup);

  test('appends input + result on submit', async () => {
    mockRpc({
      edb_evalOnSnapshot: () => ({
        Ok: { type: 'Uint', value: { bits: 256, value: '42' } },
      }),
    });
    const { wrapper } = makeWrapper();
    render(<TerminalPanel />, { wrapper });
    const input = screen.getByTestId('terminal-input');
    await userEvent.type(input, 'block.number{enter}');
    await waitFor(() => expect(screen.getByTestId('term-result')).toBeTruthy());
    expect(screen.getByTestId('term-input').textContent).toContain('block.number');
  });

  test('shows error line on RPC error', async () => {
    mockRpc({ edb_evalOnSnapshot: () => { const e: Error & { code?: number } = new Error('boom'); e.code = -33006; throw e; } });
    const { wrapper } = makeWrapper();
    render(<TerminalPanel />, { wrapper });
    await userEvent.type(screen.getByTestId('terminal-input'), 'x{enter}');
    await waitFor(() => expect(screen.getByTestId('term-error')).toBeTruthy());
  });

  test('ignores empty submit when there is no prior command', async () => {
    const { wrapper } = makeWrapper();
    render(<TerminalPanel />, { wrapper });
    await userEvent.type(screen.getByTestId('terminal-input'), '   {enter}');
    expect(useSession.getState().terminalHistory).toHaveLength(0);
  });

  test('empty submit re-runs the last command', async () => {
    let calls = 0;
    mockRpc({ edb_evalOnSnapshot: () => { calls += 1; return UINT_ONE; } });
    const { wrapper } = makeWrapper();
    render(<TerminalPanel />, { wrapper });
    const input = screen.getByTestId('terminal-input');
    await userEvent.type(input, 'block.number{enter}');
    await waitFor(() => expect(calls).toBe(1));
    await userEvent.type(input, '{enter}'); // empty → repeat
    await waitFor(() => expect(calls).toBe(2));
    const inputs = useSession.getState().terminalHistory.filter((h) => h.kind === 'input');
    expect(inputs).toHaveLength(2);
    expect(inputs[1]).toMatchObject({ text: 'block.number' });
  });

  test('ArrowUp recalls the previous command into the input', async () => {
    mockRpc({ edb_evalOnSnapshot: () => UINT_ONE });
    const { wrapper } = makeWrapper();
    render(<TerminalPanel />, { wrapper });
    const input = screen.getByTestId('terminal-input') as HTMLInputElement;
    await userEvent.type(input, 'msg.sender{enter}');
    await waitFor(() => expect(input.value).toBe(''));
    await userEvent.type(input, '{arrowup}');
    expect(input.value).toBe('msg.sender');
    await userEvent.type(input, '{arrowdown}');
    expect(input.value).toBe('');
  });

  test('autocomplete suggests command verbs and Tab completes', async () => {
    const { wrapper } = makeWrapper();
    render(<TerminalPanel />, { wrapper });
    const input = screen.getByTestId('terminal-input') as HTMLInputElement;
    await userEvent.type(input, 'cont');
    await waitFor(() => expect(screen.getByTestId('terminal-suggestions')).toBeTruthy());
    expect(
      screen.getAllByTestId('terminal-suggestion').some((o) => o.textContent === 'continue'),
    ).toBe(true);
    await userEvent.keyboard('{Tab}');
    expect(input.value).toBe('continue');
  });

  test('computeTermSuggestions: prefix match, exact + whitespace excluded', () => {
    expect(computeTermSuggestions('ed')).toContain('edb_sload(');
    expect(computeTermSuggestions('step')).toEqual([]); // exact verb, no list
    expect(computeTermSuggestions('goto 1')).toEqual([]); // past the first token
    expect(computeTermSuggestions('')).toEqual([]);
  });
});

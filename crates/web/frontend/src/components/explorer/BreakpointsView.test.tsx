import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { BreakpointsView } from './BreakpointsView';
import { useSession } from '../../store/session';
import { makeWrapper, mockRpc } from '../../hooks/_test-utils';

const ADDR = '0x' + '1'.repeat(40);

beforeEach(() => {
  useSession.setState({ breakpoints: [], currentSnapshotId: 0 });
});
afterEach(cleanup);

describe('<BreakpointsView />', () => {
  test('shows empty state', () => {
    const { wrapper } = makeWrapper();
    render(<BreakpointsView />, { wrapper });
    expect(screen.getByTestId('breakpoints-empty')).toBeTruthy();
  });

  test('lists breakpoints and supports remove', async () => {
    useSession.getState().addBreakpoint({
      loc: { kind: 'Opcode', bytecode_address: ADDR, pc: 7 },
      condition: null,
    });
    useSession.getState().addBreakpoint({
      loc: { kind: 'Source', bytecode_address: ADDR, file_path: 'X.sol', line_number: 12 },
      condition: null,
    });
    const { wrapper } = makeWrapper();
    render(<BreakpointsView />, { wrapper });
    expect(screen.getByTestId('bp-row-0')).toBeTruthy();
    expect(screen.getByTestId('bp-row-1')).toBeTruthy();

    await userEvent.click(screen.getByTestId('bp-remove-0'));
    expect(useSession.getState().breakpoints).toHaveLength(1);
  });

  test('jump to first hit calls edb_getBreakpointHits and sets snapshot', async () => {
    mockRpc({ edb_getBreakpointHits: () => [42] });
    useSession.getState().addBreakpoint({
      loc: { kind: 'Opcode', bytecode_address: ADDR, pc: 0 },
      condition: null,
    });
    const { wrapper } = makeWrapper();
    render(<BreakpointsView />, { wrapper });
    await userEvent.click(screen.getByTestId('bp-jump-0'));
    await waitFor(() => expect(useSession.getState().currentSnapshotId).toBe(42));
  });

  test('clear all empties the list', async () => {
    useSession.getState().addBreakpoint({
      loc: { kind: 'Opcode', bytecode_address: ADDR, pc: 1 },
      condition: null,
    });
    const { wrapper } = makeWrapper();
    render(<BreakpointsView />, { wrapper });
    await userEvent.click(screen.getByTestId('bp-clear-all'));
    expect(useSession.getState().breakpoints).toHaveLength(0);
  });

  test('toggle button flips the enabled flag', async () => {
    useSession.getState().addBreakpoint({
      loc: { kind: 'Opcode', bytecode_address: ADDR, pc: 1 },
      condition: null,
    });
    const { wrapper } = makeWrapper();
    render(<BreakpointsView />, { wrapper });
    expect(useSession.getState().breakpoints[0].enabled).toBe(true);
    await userEvent.click(screen.getByTestId('bp-toggle-0'));
    expect(useSession.getState().breakpoints[0].enabled).toBe(false);
  });

  test('disable-all and enable-all flip every breakpoint', async () => {
    useSession.getState().addBreakpoint({
      loc: { kind: 'Opcode', bytecode_address: ADDR, pc: 1 },
      condition: null,
    });
    useSession.getState().addBreakpoint({
      loc: { kind: 'Opcode', bytecode_address: ADDR, pc: 2 },
      condition: null,
    });
    const { wrapper } = makeWrapper();
    render(<BreakpointsView />, { wrapper });
    await userEvent.click(screen.getByTestId('bp-disable-all'));
    expect(useSession.getState().breakpoints.every((bp) => bp.enabled === false)).toBe(true);
    await userEvent.click(screen.getByTestId('bp-enable-all'));
    expect(useSession.getState().breakpoints.every((bp) => bp.enabled === true)).toBe(true);
  });

  test('typing into the condition input commits on blur', async () => {
    useSession.getState().addBreakpoint({
      loc: { kind: 'Opcode', bytecode_address: ADDR, pc: 1 },
      condition: null,
    });
    const { wrapper } = makeWrapper();
    render(<BreakpointsView />, { wrapper });
    const input = screen.getByTestId('bp-condition-0') as HTMLInputElement;
    await userEvent.type(input, 'x > 0');
    await userEvent.tab(); // blur
    await waitFor(() => expect(useSession.getState().breakpoints[0].condition).toBe('x > 0'));
  });
});

import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { FileTabPanel } from './FileTabPanel';
import { makeWrapper, mockRpc } from '../hooks/_test-utils';

const ADDR = '0x' + 'a'.repeat(40);

describe('<FileTabPanel />', () => {
  afterEach(cleanup);

  test('renders opcodes view for Opcodes payload', async () => {
    mockRpc({ edb_getCodeByAddress: () => ({ kind: 'Opcodes', disasm: '0000: PUSH1 0x60' }) });
    const { wrapper } = makeWrapper();
    render(<FileTabPanel addr={ADDR} path="<disasm>" />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('opcodes-view')).toBeTruthy());
  });

  test('renders solidity view for Source payload', async () => {
    mockRpc({
      edb_getCodeByAddress: () => ({
        kind: 'Source',
        entry: 'a.sol',
        files: [{ path: 'a.sol', content: 'contract X{}' }],
      }),
    });
    const { wrapper } = makeWrapper();
    render(<FileTabPanel addr={ADDR} path="a.sol" />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('solidity-view')).toBeTruthy());
  });

  test('accepts dockview-style params prop', async () => {
    mockRpc({ edb_getCodeByAddress: () => ({ kind: 'Opcodes', disasm: 'STOP' }) });
    const { wrapper } = makeWrapper();
    render(<FileTabPanel params={{ addr: ADDR, path: '<disasm>' }} />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('opcodes-view')).toBeTruthy());
  });
});

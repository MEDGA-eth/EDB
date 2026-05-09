import { afterEach, describe, expect, test } from 'bun:test';
import { renderHook, waitFor } from '@testing-library/react';
import { makeWrapper, mockRpc } from './_test-utils';
import { useSnapshotInfo } from './useSnapshotInfo';

const sample = {
  id: 0,
  frame_id: [0, 0],
  next_id: 1,
  prev_id: 0,
  target_address: '0x' + '0'.repeat(40),
  bytecode_address: '0x' + '0'.repeat(40),
  detail: {
    Opcode: {
      id: 0,
      frame_id: [0, 0],
      pc: 0,
      opcode: 0x60,
      memory: [],
      stack: [],
      calldata: '0x',
      transient_storage: {},
    },
  },
};

describe('useSnapshotInfo', () => {
  afterEach(() => (globalThis.fetch as { mockReset?: () => void }).mockReset?.());

  test('passes id as first param', async () => {
    let received: unknown[] = [];
    mockRpc({
      edb_getSnapshotInfo: (params) => {
        received = params!;
        return sample;
      },
    });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useSnapshotInfo(7), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(received).toEqual([7]);
    // The transform pulls the inner Opcode payload up to `kind: 'Opcode'`.
    expect(result.current.data?.detail.kind).toBe('Opcode');
  });

  test('surfaces RPC error code', async () => {
    mockRpc({
      edb_getSnapshotInfo: () => {
        const e: Error & { code?: number } = new Error('oob');
        e.code = -33001;
        throw e;
      },
    });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useSnapshotInfo(99999), { wrapper });
    await waitFor(() => expect(result.current.isError).toBe(true));
  });
});

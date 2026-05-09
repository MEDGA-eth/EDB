import { useMutation, useQueryClient } from '@tanstack/react-query';
import { rpcRaw } from '../lib/rpc';

export interface EvalArgs {
  id: number;
  expr: string;
  /** Optional cancellation signal forwarded to the underlying fetch. */
  signal?: AbortSignal;
}

export function useEvalExpr() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async ({ id, expr, signal }: EvalArgs) =>
      rpcRaw<unknown>('edb_evalOnSnapshot', [id, expr], { signal }),
    onSuccess: (data, { id, expr }) => {
      qc.setQueryData(['eval', id, expr], data);
    },
  });
}

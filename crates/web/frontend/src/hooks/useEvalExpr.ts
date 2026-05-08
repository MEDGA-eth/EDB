import { useMutation, useQueryClient } from '@tanstack/react-query';
import { rpcRaw } from '../lib/rpc';

export interface EvalArgs { id: number; expr: string }

export function useEvalExpr() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async ({ id, expr }: EvalArgs) => rpcRaw<unknown>('edb_evalOnSnapshot', [id, expr]),
    onSuccess: (data, { id, expr }) => {
      qc.setQueryData(['eval', id, expr], data);
    },
  });
}

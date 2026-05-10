import { useMutation, useQueryClient } from '@tanstack/react-query';
import { rpc } from '../lib/rpc';
import { EvalResult } from '../lib/types';
import type { EvalResult as EvalResultType } from '../lib/types';

export interface EvalArgs {
  id: number;
  expr: string;
  /** Optional cancellation signal forwarded to the underlying fetch. */
  signal?: AbortSignal;
}

/**
 * Wrapper around `edb_evalOnSnapshot`. The engine returns
 * `Result<EdbSolValue, String>` which we transform into
 * `{ kind: 'Ok', value } | { kind: 'Err', error }`. Errors emitted by the
 * engine itself (Solidity parse errors, type mismatches) come back as
 * `kind: 'Err'`, NOT as RPC errors, the RPC layer only throws for
 * transport / serialisation failures.
 */
export function useEvalExpr() {
  const qc = useQueryClient();
  return useMutation<EvalResultType, Error, EvalArgs>({
    mutationFn: async ({ id, expr, signal }) =>
      rpc('edb_evalOnSnapshot', EvalResult, [id, expr], { signal }),
    onSuccess: (data, { id, expr }) => {
      qc.setQueryData(['eval', id, expr], data);
    },
  });
}

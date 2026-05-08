import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import type { ReactElement, ReactNode } from 'react';

export function makeWrapper(): {
  wrapper: ({ children }: { children: ReactNode }) => ReactElement;
  client: QueryClient;
} {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity } },
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return { wrapper, client };
}

export function mockRpc(handlers: Record<string, (params?: unknown[]) => unknown>) {
  globalThis.fetch = (async (_url: unknown, init: unknown) => {
    const body = (init as RequestInit).body as string;
    const req = JSON.parse(body) as { method: string; params?: unknown[]; id: number };
    const handler = handlers[req.method];
    if (!handler) {
      return new Response(
        JSON.stringify({
          jsonrpc: '2.0',
          id: req.id,
          error: { code: -32601, message: 'no mock' },
        }),
      );
    }
    try {
      const result = handler(req.params);
      return new Response(JSON.stringify({ jsonrpc: '2.0', id: req.id, result }));
    } catch (e) {
      const err = e as { code?: number; message?: string };
      return new Response(
        JSON.stringify({
          jsonrpc: '2.0',
          id: req.id,
          error: { code: err.code ?? -33000, message: err.message ?? 'mock error' },
        }),
      );
    }
  }) as unknown as typeof fetch;
}

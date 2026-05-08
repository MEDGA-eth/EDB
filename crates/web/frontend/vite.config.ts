import { defineConfig, type ProxyOptions } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import path from 'node:path';
import { MOCKS } from './src/data/mocks';

function mockMiddleware() {
  return {
    name: 'edb-rpc-mock',
    configureServer(server: { middlewares: { use: (path: string, fn: (req: any, res: any, next: any) => void) => void } }) {
      server.middlewares.use('/health', (_req: any, res: any) => {
        res.setHeader('content-type', 'application/json');
        res.end(JSON.stringify({ status: 'healthy' }));
      });
      server.middlewares.use('/', (req: any, res: any, next: any) => {
        if (req.method !== 'POST') return next();
        let body = '';
        req.on('data', (c: Buffer) => {
          body += c;
        });
        req.on('end', () => {
          try {
            const { method, params, id } = JSON.parse(body);
            const handler = MOCKS[method];
            const payload = handler
              ? { jsonrpc: '2.0', id, result: handler(params) }
              : { jsonrpc: '2.0', id, error: { code: -32601, message: `unknown ${method}` } };
            res.setHeader('content-type', 'application/json');
            res.end(JSON.stringify(payload));
          } catch (e) {
            res.statusCode = 400;
            res.setHeader('content-type', 'application/json');
            res.end(
              JSON.stringify({
                jsonrpc: '2.0',
                id: null,
                error: { code: -32700, message: `parse error: ${(e as Error).message}` },
              }),
            );
          }
        });
      });
    },
  };
}

export default defineConfig(({ mode }) => {
  const port = Number(process.env.VITE_EDB_RPC_PORT ?? 8545);
  const proxy: Record<string, string | ProxyOptions> = mode === 'mock' ? {} : {
    '^/$': {
      target: `http://127.0.0.1:${port}`,
      changeOrigin: false,
      bypass(req: { method?: string; url?: string }) {
        if (req.method === 'POST') return undefined;
        return req.url;
      },
    },
    '/health': { target: `http://127.0.0.1:${port}`, changeOrigin: false },
  };

  return {
    plugins: [react(), tailwindcss(), ...(mode === 'mock' ? [mockMiddleware()] : [])],
    resolve: { alias: { '@': path.resolve(__dirname, 'src') } },
    server: { proxy },
    build: { outDir: 'dist', emptyOutDir: true, sourcemap: true },
  };
});

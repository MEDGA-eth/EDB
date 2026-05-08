import { defineConfig, type ProxyOptions } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import path from 'node:path';

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
    plugins: [react(), tailwindcss()],
    resolve: { alias: { '@': path.resolve(__dirname, 'src') } },
    server: { proxy },
    build: { outDir: 'dist', emptyOutDir: true, sourcemap: true },
  };
});

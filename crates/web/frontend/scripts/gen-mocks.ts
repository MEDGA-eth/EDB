import { writeFile } from 'node:fs/promises';
import path from 'node:path';

const port = process.env.VITE_EDB_RPC_PORT ?? '8545';
const url = `http://127.0.0.1:${port}/`;

async function rpc(method: string, params?: unknown[]) {
  const r = await fetch(url, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ jsonrpc: '2.0', method, params, id: 1 }),
  });
  const j = await r.json();
  if (j.error) throw new Error(`${method}: ${j.error.message}`);
  return j.result;
}

const out = (name: string, data: unknown) =>
  writeFile(path.join('src/data/mocks', name), JSON.stringify(data, null, 2));

await out('snapshotCount.json', await rpc('edb_getSnapshotCount'));
await out('trace.json', await rpc('edb_getTrace'));
await out('snapshotInfo-0.json', await rpc('edb_getSnapshotInfo', [0]));
await out('code-0.json', await rpc('edb_getCode', [0]));
await out('storageDiff-0.json', await rpc('edb_getStorageDiff', [0]));

console.log('Mocks regenerated.');

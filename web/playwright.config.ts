import { createServer } from 'node:net';
import { defineConfig } from '@playwright/test';

async function freePort(): Promise<number> {
  const server = createServer();
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const address = server.address();
  const port = typeof address === 'object' && address !== null ? address.port : 0;
  await new Promise((resolve) => server.close(resolve));
  return port;
}

// Workers re-load this file; they inherit the port the first load chose through the environment.
process.env['CODEMAP_E2E_PORT'] ??= String(await freePort());
const port = process.env['CODEMAP_E2E_PORT'];

export default defineConfig({
  testDir: 'e2e',
  forbidOnly: true,
  workers: 6,
  use: { baseURL: `http://127.0.0.1:${port}`, reducedMotion: 'reduce' },
  // The binary as `mise run check` just built it, with the web build embedded.
  webServer: {
    command: `../target/debug/codemap serve --root .. --port ${port}`,
    url: `http://127.0.0.1:${port}`,
  },
});

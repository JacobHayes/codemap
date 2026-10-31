import { execFileSync } from 'node:child_process';
import { createServer } from 'node:net';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from '@playwright/test';

async function freePort(): Promise<number> {
  const server = createServer();
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const address = server.address();
  const port = typeof address === 'object' && address !== null ? address.port : 0;
  await new Promise((resolve) => server.close(resolve));
  return port;
}

// The debug binary lives in cargo's target directory, wherever CARGO_TARGET_DIR puts it.
function binary(): string {
  const metadata = execFileSync('cargo', ['metadata', '--format-version', '1', '--no-deps'], {
    cwd: fileURLToPath(new URL('..', import.meta.url)),
    encoding: 'utf8',
    maxBuffer: 1 << 26,
  });
  return join(
    (JSON.parse(metadata) as { target_directory: string }).target_directory,
    'debug',
    'codemap',
  );
}

function shellQuote(value: string): string {
  return `'${value.replaceAll("'", `'\\''`)}'`;
}

// Workers re-load this file; they inherit the port the first load chose through the environment.
process.env['CODEMAP_E2E_PORT'] ??= String(await freePort());
const port = process.env['CODEMAP_E2E_PORT'];
process.env['CODEMAP_E2E_BIN'] ??= binary();

export default defineConfig({
  testDir: 'e2e',
  forbidOnly: true,
  workers: 6,
  use: { baseURL: `http://127.0.0.1:${port}`, reducedMotion: 'reduce' },
  // The binary as `mise run check` (or `cargo build`) just built it; never rebuilt here, with the web build embedded.
  webServer: {
    command: `${shellQuote(process.env['CODEMAP_E2E_BIN'] ?? '')} serve --root .. --port ${port}`,
    url: `http://127.0.0.1:${port}`,
  },
});

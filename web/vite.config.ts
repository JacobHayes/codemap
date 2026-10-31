import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

// `npm run dev` proxies the API to a running `codemap serve` (its default port).
export default defineConfig({
  plugins: [react()],
  server: {
    // The shared design assets live beside web/, outside the Vite root.
    fs: { allow: ['..'] },
    proxy: { '/api': 'http://127.0.0.1:7878' },
  },
  test: { environment: 'jsdom', include: ['src/**/*.test.{ts,tsx}'] },
});

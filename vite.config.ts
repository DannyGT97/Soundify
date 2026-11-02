import { defineConfig } from 'vite';

// Tauri serves the dev page to the desktop window and, on Android, to the device
// over the network: `TAURI_DEV_HOST` is set by `tauri android dev`.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  clearScreen: false,
  server: {
    port: 5180,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 5181 } : undefined,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  build: { target: 'es2022' },
});

import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

// https://vite.dev/config/
// The web/PWA build uses absolute asset URLs so deep links such as /help/security
// and /backbone load correctly. Tauri builds keep relative URLs for its custom protocol.
const isTauriBuild = Boolean(process.env.TAURI_ENV_PLATFORM || process.env.TAURI_ENV_ARCH);

export default defineConfig({
  base: isTauriBuild ? './' : '/',
  plugins: [react(), tailwindcss()],
  server: {
    port: 5173,
    strictPort: false,
    host: '127.0.0.1',
  },
  // MapLibre GL JS v6 loads its worker as an ES module; Vite's dep optimizer
  // rewrites the worker URL and breaks it in dev, so keep maplibre out of it.
  optimizeDeps: {
    exclude: ['maplibre-gl'],
  },
  build: {
    target: 'es2022',
    sourcemap: false,
    chunkSizeWarningLimit: 1400,
    rollupOptions: {
      output: {
        manualChunks: {
          maplibre: ['maplibre-gl'],
          react: ['react', 'react-dom'],
        },
      },
    },
  },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts', 'src/**/*.test.tsx'],
    reporters: 'default',
  },
});

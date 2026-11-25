import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

// https://vite.dev/config/
// The web/PWA build uses absolute asset URLs so deep links such as /help/security
// and /backbone load correctly. Tauri builds keep relative URLs for its custom protocol.
const isTauriBuild = Boolean(process.env.TAURI_ENV_PLATFORM || process.env.TAURI_ENV_ARCH);

export default defineConfig({
  base: isTauriBuild ? './' : '/',
  plugins: [react()],
  server: {
    port: 5173,
    strictPort: false,
    host: '127.0.0.1',
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

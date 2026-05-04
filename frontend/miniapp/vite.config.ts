import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    // Telegram serves the Mini App over HTTPS; use a tunnel (e.g. ngrok) in dev.
    host: true,
  },
  build: {
    outDir: 'dist',
    sourcemap: true,
  },
});

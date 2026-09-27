import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

const isTauriPath = (path: string) => /(^|[\\/])src-tauri([\\/]|$)/i.test(path);

export default defineConfig({
  plugins: [react()],
  server: {
    watch: {
      ignored: isTauriPath,
      usePolling: true,
    },
    fs: {
      deny: ['**/src-tauri/**'],
    },
  },
});
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  server: {
    port: 4173,
    proxy: { '/v1': { target: process.env.FORGE_API_TARGET ?? 'http://127.0.0.1:3100' } },
  },
  resolve: { dedupe: ['react', 'react-dom'] },
});

import { defineConfig } from 'vite';
import solid from 'vite-plugin-solid';

// Tauri 固定端口；浏览器直接打开时 invoke 不可用，UI 会提示需在桌面端运行。
export default defineConfig({
  plugins: [solid()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    target: 'es2020',
  },
});

import { defineConfig, loadEnv } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig(({ mode }) => {
  // 用 loadEnv 而不是 process.env —— 后者要拖一个 @types/node 进来，就为了读一个环境变量。
  const env = loadEnv(mode, '.', '');
  const host = env.TAURI_DEV_HOST;

  return {
    plugins: [svelte()],
    clearScreen: false,
    server: {
      port: 1420,
      strictPort: true,
      host: host || false,
      hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
      watch: { ignored: ['**/src-tauri/**'] }
    },
    envPrefix: ['VITE_', 'TAURI_'],
    build: {
      target: 'chrome110',
      minify: 'esbuild',
      sourcemap: false,
      // CodeMirror + highlight.js 加起来就 1.7MB 了。这是桌面应用，资源从本地磁盘读，
      // 不经网络，所以体积警告在这儿没有实际意义 —— 与其每次构建都喊一嗓子，不如把阈值调对。
      chunkSizeWarningLimit: 2000
    }
  };
});

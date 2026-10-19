import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'

// https://vite.dev/config/
export default defineConfig({
  clearScreen: false,
  server: {
    host: 'localhost',
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  plugins: [react(), tailwindcss()],
  build: {
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [
            { name: 'vendor-react', test: /node_modules[\\/](react|react-dom|scheduler)[\\/]/, priority: 40 },
            { name: 'vendor-radix', test: /node_modules[\\/](radix-ui|@radix-ui|@floating-ui|aria-hidden|react-remove-scroll[^\\/]*|react-style-singleton|use-callback-ref|use-sidecar|get-nonce)[\\/]/, priority: 30 },
            { name: 'vendor-cmdk', test: /node_modules[\\/]cmdk[\\/]/, priority: 30 },
            { name: 'vendor-zod', test: /node_modules[\\/]zod[\\/]/, priority: 30 },
            { name: 'vendor-icons', test: /node_modules[\\/]lucide-react[\\/]/, priority: 30 },
          ],
        },
      },
    },
  },
  resolve: {
    alias: {
      '@': new URL('./src', import.meta.url).pathname,
    },
  },
})

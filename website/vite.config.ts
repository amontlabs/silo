import { fileURLToPath, URL } from 'node:url';
import react from '@vitejs/plugin-react';
import type { Plugin } from 'vite';
import { defineConfig } from 'vitest/config';
import { DECISIONS_FILE, FEATURES_FILE, readBuildData, STAGES_FILE } from './src/workshop/data.ts';

const repository = fileURLToPath(new URL('..', import.meta.url));
const recordings = fileURLToPath(new URL('./public/workshop/recordings', import.meta.url));

/** `virtual:workshop-data`: the plan from docs/product, parsed and checked at build time. */
function workshopData(): Plugin {
  const id = 'virtual:workshop-data';
  return {
    name: 'workshop-data',
    resolveId: (source) => (source === id ? `\0${id}` : null),
    load(source) {
      if (source !== `\0${id}`) return null;
      for (const file of [FEATURES_FILE, STAGES_FILE, DECISIONS_FILE]) this.addWatchFile(`${repository}${file}`);
      return `export default ${JSON.stringify(readBuildData(repository, recordings))};`;
    },
  };
}

export default defineConfig({
  plugins: [react(), workshopData()],
  build: {
    rolldownOptions: {
      input: {
        main: fileURLToPath(new URL('./index.html', import.meta.url)),
        workshop: fileURLToPath(new URL('./workshop/index.html', import.meta.url)),
      },
    },
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts', 'src/**/*.test.tsx'],
    setupFiles: ['src/workshop/test-setup.ts'],
  },
});

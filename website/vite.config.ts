import { fileURLToPath, URL } from 'node:url';
import react from '@vitejs/plugin-react';
import type { Plugin } from 'vite';
import { defineConfig } from 'vitest/config';
import { planDir, readBuildData, readNotes } from './src/workshop/data.ts';

const repository = fileURLToPath(new URL('..', import.meta.url));
// plan/, or the plan WORKSHOP_PLAN names: a fixture plan proves the page without the real one.
const plan = planDir(repository);

/** `virtual:workshop-data`: the plan, read with tools/plan/read and checked with tools/plan/check; any error stops the build. */
function workshopData(): Plugin {
  const id = 'virtual:workshop-data';
  return {
    name: 'workshop-data',
    resolveId: (source) => (source === id ? `\0${id}` : null),
    load(source) {
      if (source !== `\0${id}`) return null;
      return `export default ${JSON.stringify(readBuildData(repository))};`;
    },
  };
}

/** `virtual:workshop-notes`: the notes of every entry, a chunk of their own that the page loads on demand. */
function workshopNotes(): Plugin {
  const id = 'virtual:workshop-notes';
  return {
    name: 'workshop-notes',
    resolveId: (source) => (source === id ? `\0${id}` : null),
    load(source) {
      if (source !== `\0${id}`) return null;
      return `export default ${JSON.stringify(readNotes(repository))};`;
    },
  };
}

/** While developing, the page reloads with the new data when a file of the plan is added, changed or removed. */
function planWatch(): Plugin {
  return {
    name: 'plan-watch',
    configureServer(server) {
      server.watcher.add(plan);
      server.watcher.on('all', (_event, file) => {
        if (!file.startsWith(plan)) return;
        for (const id of ['\0virtual:workshop-data', '\0virtual:workshop-notes']) {
          const module = server.moduleGraph.getModuleById(id);
          if (module) server.moduleGraph.invalidateModule(module);
        }
        server.ws.send({ type: 'full-reload' });
      });
    },
  };
}

export default defineConfig({
  plugins: [react(), workshopData(), workshopNotes(), planWatch()],
  // The page imports the plan's model from tools/plan/model, outside website/.
  server: { fs: { allow: [repository] } },
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

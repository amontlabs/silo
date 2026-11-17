import { join } from 'node:path';
import { fillHomePlan } from './src/home-plan.ts';
import { fileURLToPath, URL } from 'node:url';
import react from '@vitejs/plugin-react';
import type { Plugin } from 'vite';
import { defineConfig } from 'vitest/config';
import { PLAN_DIR, readBuildData } from './src/workshop/data.ts';

const repository = fileURLToPath(new URL('..', import.meta.url));
const plan = join(repository, PLAN_DIR);

/** `virtual:workshop-data`: the plan from plan/, read with tools/plan/read and checked with tools/plan/check; any error stops the build. */
function workshopData(): Plugin {
  const id = 'virtual:workshop-data';
  return {
    name: 'workshop-data',
    resolveId: (source) => (source === id ? `\0${id}` : null),
    load(source) {
      if (source !== `\0${id}`) return null;
      this.addWatchFile(plan);
      return `export default ${JSON.stringify(readBuildData(repository))};`;
    },
  };
}

/** Fills the home page's "First up" and Workshop lines from the plan at build time. */
function homePlan(): Plugin {
  const home = fileURLToPath(new URL('./index.html', import.meta.url));
  return {
    name: 'home-plan',
    configureServer(server) {
      server.watcher.add(plan);
      server.watcher.on('change', (file) => {
        if (file.startsWith(plan)) server.ws.send({ type: 'full-reload', path: '/index.html' });
      });
    },
    transformIndexHtml: {
      order: 'pre',
      handler: (html, context) => (context.filename === home ? fillHomePlan(html, repository) : html),
    },
  };
}

/** While developing, the data reloads when a file of plan/ changes. */
function planWatch(): Plugin {
  return {
    name: 'plan-watch',
    configureServer(server) {
      server.watcher.add(plan);
      server.watcher.on('change', (file) => {
        if (!file.startsWith(plan)) return;
        const module = server.moduleGraph.getModuleById('\0virtual:workshop-data');
        if (module) server.moduleGraph.invalidateModule(module);
      });
    },
  };
}

export default defineConfig({
  plugins: [react(), workshopData(), homePlan(), planWatch()],
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

import { fileURLToPath, URL } from 'node:url';
import react from '@vitejs/plugin-react';
import type { Plugin } from 'vite';
import { defineConfig } from 'vitest/config';
import { createReadStream } from 'node:fs';
import { extname, resolve } from 'node:path';
import { planDir, readBuildData, readFacts, readHistories, readNotes } from './src/workshop/data.ts';
import { FILMS_URL, filmFileFor, filmFiles, writeFilms } from './scripts/films.mjs';

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

/** `virtual:workshop-facts`: the statements in force and what each entry says of them, a chunk of their own that the page loads when a row is first opened. */
function workshopFacts(): Plugin {
  const id = 'virtual:workshop-facts';
  return {
    name: 'workshop-facts',
    resolveId: (source) => (source === id ? `\0${id}` : null),
    load(source) {
      if (source !== `\0${id}`) return null;
      return `export default ${JSON.stringify(readFacts(repository))};`;
    },
  };
}

/** `virtual:workshop-history`: the changes of every entry, read from git, a chunk of their own that the page loads when a History is first opened. */
function workshopHistory(): Plugin {
  const id = 'virtual:workshop-history';
  return {
    name: 'workshop-history',
    resolveId: (source) => (source === id ? `\0${id}` : null),
    load(source) {
      if (source !== `\0${id}`) return null;
      return `export default ${JSON.stringify(readHistories(repository))};`;
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
        for (const id of ['\0virtual:workshop-data', '\0virtual:workshop-notes', '\0virtual:workshop-facts', '\0virtual:workshop-history']) {
          const module = server.moduleGraph.getModuleById(id);
          if (module) server.moduleGraph.invalidateModule(module);
        }
        server.ws.send({ type: 'full-reload' });
      });
    },
  };
}

const FILM_TYPES: Record<string, string> = {
  '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8',
  '.mp3': 'audio/mpeg', '.otf': 'font/otf', '.ttf': 'font/ttf', '.txt': 'text/plain; charset=utf-8',
};

/** The films of films/ at /films/<id>/ (scripts/films.mjs): written beside the built site, and served as they are while developing. */
function films(): Plugin {
  let outDir = '';
  return {
    name: 'films',
    configResolved(config) {
      outDir = resolve(config.root, config.build.outDir);
    },
    async writeBundle() {
      const ids = await writeFilms(outDir);
      this.info(`wrote ${ids.length} films into ${outDir}/films`);
    },
    configureServer(server) {
      server.middlewares.use((request, response, next) => {
        const pathname = (request.url ?? '').split('?')[0];
        if (!pathname.startsWith(FILMS_URL)) return next();
        const file = filmFileFor(filmFiles().files, decodeURIComponent(pathname));
        if (!file) return next();
        const type = 'text' in file ? FILM_TYPES['.html'] : FILM_TYPES[extname(file.from)] ?? 'application/octet-stream';
        response.setHeader('Content-Type', type);
        if ('text' in file) response.end(file.text);
        else createReadStream(file.from).pipe(response);
      });
    },
  };
}

export default defineConfig({
  plugins: [react(), workshopData(), workshopNotes(), workshopFacts(), workshopHistory(), planWatch(), films()],
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

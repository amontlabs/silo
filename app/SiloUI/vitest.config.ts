import react from "@vitejs/plugin-react"
import { defineConfig } from "vitest/config"

// Only suites that need no browser APIs belong here. Unclassified suites retain jsdom
// until their imports and behavior have been checked, including lifecycle hooks.
const nodeTests = [
  "src/contracts/**/*.test.ts",
  "src/fixtures/{onboarding-handoff,settings,surfaces}.test.ts",
  "src/desktop/*-permissions.test.ts",
  "src/desktop/{applications,production-entry,production-onboarding,system-integrations,updates}.test.ts",
  "src/features/application/model/**/*.test.ts",
  "src/features/onboarding/model/**/*.test.ts",
  "src/features/preferences/model/**/*.test.ts",
  "src/features/preferences/{settings-store,system-integrations-store}.test.ts",
  "src/test/{git-runtime,microsandbox-runtime,native-permissions,transition-styles}.test.ts",
]

// Share transforms and aliases only. Inheriting maxWorkers into each project
// would override the root CLI limit used by CI (for example --maxWorkers=2).
const projectConfig = {
  plugins: [react()],
  resolve: {
    alias: { "@": new URL("./src", import.meta.url).pathname },
  },
}

// Restore spies, stubbed globals and stubbed env after every test so one
// test's replacements cannot leak into the next. Timers have no config switch;
// src/test/setup-common.ts restores real timers after every test instead.
const testHygiene = {
  globals: true,
  restoreMocks: true,
  unstubGlobals: true,
  unstubEnvs: true,
}

export default defineConfig({
  test: {
    // Full-window interaction tests compete for CPU when every jsdom file runs
    // at once. Keep their normal timeout meaningful under the complete suite.
    maxWorkers: 4,
    projects: [
      {
        ...projectConfig,
        test: {
          ...testHygiene,
          name: "node",
          include: nodeTests,
          environment: "node",
          setupFiles: ["./src/test/setup-common.ts"],
        },
      },
      {
        ...projectConfig,
        test: {
          ...testHygiene,
          name: "dom",
          include: ["src/**/*.test.{ts,tsx}"],
          exclude: nodeTests,
          environment: "jsdom",
          // Node 25+ exposes its own localStorage global, which is undefined
          // without --localstorage-file and shadows the jsdom storage.
          execArgv: ["--no-experimental-webstorage"],
          setupFiles: ["./src/test/setup-common.ts", "./src/test/setup.ts"],
          css: true,
        },
      },
    ],
  },
})

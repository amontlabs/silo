import { readFileSync } from "node:fs"
import { dirname, extname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { describe, expect, it } from "vitest"

const sourceRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..")

function resolveSource(specifier: string, importer: string): string | null {
  const base = specifier.startsWith("@/")
    ? resolve(sourceRoot, specifier.slice(2))
    : specifier.startsWith(".") ? resolve(dirname(importer), specifier) : ""
  if (!base) return null
  for (const candidate of [base, `${base}.ts`, `${base}.tsx`, resolve(base, "index.ts"), resolve(base, "index.tsx")]) {
    try { if ([".ts", ".tsx"].includes(extname(candidate)) && readFileSync(candidate, "utf8")) return candidate } catch { /* not a source module */ }
  }
  return null
}

function productionGraph(entry: string): string[] {
  const seen = new Set<string>()
  function visit(file: string) {
    if (seen.has(file)) return
    seen.add(file)
    const text = readFileSync(file, "utf8")
    for (const match of text.matchAll(/(?:(?:import|export)\s+(?:type\s+)?(?:[^"']*?\s+from\s+)?["']([^"']+)["']|import\(\s*["']([^"']+)["']\s*\))/g)) {
      const dependency = resolveSource(match[1] ?? match[2], file)
      if (dependency) visit(dependency)
    }
  }
  visit(entry)
  return [...seen]
}

describe("production entry graph", () => {
  it("contains no fixture or scenario modules and no fixture launch path", () => {
    const entry = resolve(sourceRoot, "main.tsx")
    const graph = productionGraph(entry)
    expect(graph.filter((file) => file.includes("/fixtures/"))).toEqual([])
    const source = graph.map((file) => readFileSync(file, "utf8")).join("\n")
    expect(source).not.toContain("SILO_DEBUG_FIXTURE_QUERY")
    expect(readFileSync(entry, "utf8")).not.toMatch(/FixtureApp|scenarioFromSearch|applicationSourceForScenario|window\.location\.search/)
  })
})

import { readFileSync, readdirSync } from "node:fs"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { expect, it } from "vitest"

const native = resolve(dirname(fileURLToPath(import.meta.url)), "../../src-tauri")
type Capability = { windows?: string[]; webviews?: string[]; remote?: unknown; permissions: Array<string | { identifier: string }> }
const capabilities = readdirSync(resolve(native, "capabilities")).filter(name => name.endsWith(".json"))
  .map(name => JSON.parse(readFileSync(resolve(native, "capabilities", name), "utf8")) as Capability)
function matches(pattern: string, label: string) {
  return new RegExp(`^${pattern.split("*").map(part => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join(".*")}$`).test(label)
}
function permissions(window: string, webview: string) {
  return capabilities.filter(capability => capability.windows?.some(pattern => matches(pattern, window)) || capability.webviews?.some(pattern => matches(pattern, webview)))
    .flatMap(capability => capability.permissions.map(permission => typeof permission === "string" ? permission : permission.identifier))
}

it("grants the desktop shell only its desktop controls, native menu, and fullscreen", () => {
  const allowed = permissions("desktop-shell-example", "desktop-shell-example")
  expect(allowed.sort()).toEqual([
    "allow-read-desktop-state", "allow-desktop-action", "allow-desktop-viewer-attach", "allow-desktop-viewer-detach",
    "core:window:allow-is-fullscreen", "core:window:allow-set-fullscreen",
    "core:window:allow-inner-size", "core:window:allow-scale-factor",
    "core:menu:allow-new", "core:menu:allow-popup", "core:resources:allow-close",
    "allow-upload-files", "allow-cancel-transfer", "core:event:allow-listen", "core:event:allow-unlisten",
  ].sort())
  expect(permissions("main", "main")).toContain("allow-open-desktop")
})

it("does not grant guest content native commands even though it shares the shell window", () => {
  expect(permissions("desktop-shell-example", "guest-desktop-shell-example")).toEqual([])
  expect(capabilities.filter(capability => capability.remote)).toEqual([])
})

it("registers the viewer commands in both native dispatch and build permissions", () => {
  const main = readFileSync(resolve(native, "src/main.rs"), "utf8")
  const build = readFileSync(resolve(native, "build.rs"), "utf8")
  for (const command of ["read_desktop_state", "desktop_action", "open_desktop", "desktop_viewer_attach", "desktop_viewer_detach"]) {
    expect(main).toContain(`::${command},`)
    expect(build).toContain(`"${command}"`)
  }
})

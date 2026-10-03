import { readFileSync, readdirSync } from "node:fs"
import { resolve } from "node:path"
import { describe, expect, it } from "vitest"

const native = resolve(import.meta.dirname, "../../src-tauri")
const commands = ["choose_upload_files", "upload_files", "download_file", "cancel_transfer"]
type Capability = { identifier: string; windows: string[]; webviews?: string[]; permissions: Array<string | { identifier: string }> }
const files = readdirSync(resolve(native, "capabilities")).filter(name => name.endsWith(".json"))
  .map(name => JSON.parse(readFileSync(resolve(native, "capabilities", name), "utf8")) as Capability)
// With an explicit list in tauri.conf.json, only the listed capabilities apply.
const configured = (JSON.parse(readFileSync(resolve(native, "tauri.conf.json"), "utf8")) as { app: { security: { capabilities: string[] } } }).app.security.capabilities
const capabilities = files.filter(capability => configured.includes(capability.identifier))
const identifier = (entry: string | { identifier: string }) => typeof entry === "string" ? entry : entry.identifier
const holders = (permission: string) => capabilities.filter(capability => capability.permissions.some(entry => identifier(entry) === permission))
const permission = (command: string) => `allow-${command.replaceAll("_", "-")}`

describe("file transfer permission boundary", () => {
  it("applies every capability file and only existing ones", () => {
    expect([...configured].sort()).toEqual(files.map(capability => capability.identifier).sort())
  })

  it.each(commands)("registers %s with the native manifest and handler list", command => {
    const manifest = readFileSync(resolve(native, "build.rs"), "utf8").split(".commands(&[")[1]?.split("])")[0] ?? ""
    expect(manifest).toContain(`"${command}"`)
    expect(readFileSync(resolve(native, "src/main.rs"), "utf8")).toContain(`transfer::${command},`)
  })

  it.each(["choose_upload_files", "download_file"])("lets only the main window use %s", command => {
    const granted = holders(permission(command))
    expect(granted.flatMap(capability => capability.windows)).toEqual(["main"])
    expect(granted.flatMap(capability => capability.webviews ?? [])).toEqual([])
  })

  it.each(["upload_files", "cancel_transfer"])("lets only the main window and desktop viewer shells use %s", command => {
    const granted = holders(permission(command))
    expect(granted.flatMap(capability => capability.windows)).toEqual(["main"])
    expect(granted.flatMap(capability => capability.webviews ?? [])).toEqual(["desktop-shell-*"])
  })

  it("never grants transfers to the status panel or to a wildcard target", () => {
    for (const command of commands) {
      for (const capability of holders(permission(command))) {
        expect(capability.windows).not.toContain("status")
        expect(capability.windows).not.toContain("*")
        expect(capability.webviews ?? []).not.toContain("*")
      }
    }
  })

  it("lets the viewer shell listen for transfer progress", () => {
    const viewer = capabilities.find(capability => capability.identifier === "desktop-transfer")
    expect(viewer?.webviews).toEqual(["desktop-shell-*"])
    expect((viewer?.permissions ?? []).map(identifier)).toEqual(expect.arrayContaining(["core:event:allow-listen", "core:event:allow-unlisten"]))
  })
})

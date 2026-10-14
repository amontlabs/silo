import { readFileSync, readdirSync } from "node:fs"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { describe, expect, it } from "vitest"

const native = resolve(dirname(fileURLToPath(import.meta.url)), "../../src-tauri")
const commands = ["read_network_state", "save_network_port", "remove_network_port", "open_network_port"]
type Capability = { windows: string[]; permissions: Array<string | { identifier: string }> }
const capabilities = readdirSync(resolve(native, "capabilities")).filter((name) => name.endsWith(".json"))
  .map((name) => JSON.parse(readFileSync(resolve(native, "capabilities", name), "utf8")) as Capability)

function permissions(window: string) {
  return capabilities.filter((capability) => capability.windows.some((pattern) => pattern === window || pattern === "*"))
    .flatMap((capability) => capability.permissions.map((permission) => typeof permission === "string" ? permission : permission.identifier))
}

describe("Network desktop permission boundary", () => {
  // Mocked invokes cannot detect a command present in Rust but denied by Tauri.
  it("lets the main window reach every Network command through the native permission manifest", () => {
    const handlers = readFileSync(resolve(native, "src/main.rs"), "utf8").split("tauri::generate_handler![")[1]?.split("])")[0] ?? ""
    const manifest = readFileSync(resolve(native, "build.rs"), "utf8").split(".commands(&[")[1]?.split("])")[0] ?? ""
    const allowed = permissions("main")
    for (const command of commands) {
      expect(handlers).toContain(`network::${command},`)
      expect(manifest).toContain(`"${command}"`)
      expect(allowed).toContain(`allow-${command.replaceAll("_", "-")}`)
    }
  })

  it("lets the status panel read and open services without changing exposure", () => {
    const allowed = permissions("status")
    for (const command of ["read_network_state", "open_network_port"]) {
      expect(allowed).toContain(`allow-${command.replaceAll("_", "-")}`)
    }
    for (const command of ["save_network_port", "remove_network_port"]) {
      expect(allowed).not.toContain(`allow-${command.replaceAll("_", "-")}`)
    }
  })
})

it("keeps pending navigation exclusive to main and allows status push dismissal", () => {
  expect(permissions("main")).toContain("allow-take-main-route")
  expect(permissions("status")).not.toContain("allow-take-main-route")
  expect(permissions("status")).toContain("allow-dismiss-repository-push")
  const handlers = readFileSync(resolve(native, "src/main.rs"), "utf8")
  const manifest = readFileSync(resolve(native, "build.rs"), "utf8")
  for (const command of ["take_main_route", "dismiss_repository_push"]) {
    expect(handlers).toContain(`::${command},`)
    expect(manifest).toContain(`"${command}"`)
  }
})


it("allows SSH management only in the main window", () => {
  const handlers = readFileSync(resolve(native, "src/main.rs"), "utf8")
  const manifest = readFileSync(resolve(native, "build.rs"), "utf8")
  for (const command of ["read_ssh_access_state", "save_ssh_access"]) {
    expect(handlers).toContain(`ssh_access::${command},`)
    expect(manifest).toContain(`"${command}"`)
    expect(permissions("main")).toContain(`allow-${command.replaceAll("_", "-")}`)
    expect(permissions("status")).not.toContain(`allow-${command.replaceAll("_", "-")}`)
  }
})


it("allows remote SSH management only in the main window", () => {
  const handlers = readFileSync(resolve(native, "src/main.rs"), "utf8")
  const manifest = readFileSync(resolve(native, "build.rs"), "utf8")
  for (const command of ["remote_ssh_access_state", "remote_save_ssh_access"]) {
    expect(handlers).toContain(`commands::remote::${command},`)
    expect(manifest).toContain(`"${command}"`)
    expect(permissions("main")).toContain(`allow-${command.replaceAll("_", "-")}`)
    expect(permissions("status")).not.toContain(`allow-${command.replaceAll("_", "-")}`)
  }
})

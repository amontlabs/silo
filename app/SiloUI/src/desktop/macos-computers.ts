import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { createMacosComputersStore, type MacosComputersBackend } from "@/features/macos-computers/model/macos-computers"

export const nativeMacosComputersBackend: MacosComputersBackend = {
  read: () => invoke("read_macos_computers"),
  create: request => invoke("create_macos_computer", { request }),
  action: (id, action) => invoke("macos_computer_action", { id, action }),
  openDisplay: id => invoke("open_macos_display", { id }),
  clipboard: (id, direction) => invoke("macos_computer_clipboard", { id, direction }),
  listen: handler => listen("silo://macos-computers-changed", event => handler(event.payload)),
}

export const nativeMacosComputersStore = createMacosComputersStore(nativeMacosComputersBackend)

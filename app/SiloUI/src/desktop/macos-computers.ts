import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { createMacosComputersStore, type MacosComputersBackend } from "@/features/macos-computers/model/macos-computers"

export const nativeMacosComputersBackend: MacosComputersBackend = {
  read: () => invoke("read_macos_computers"),
  create: request => invoke("create_macos_computer", { request }),
  action: (id, action) => invoke("macos_computer_action", { id, action }),
  openDisplay: id => invoke("open_macos_display", { id }),
  clipboard: (id, direction) => invoke("macos_computer_clipboard", { id, direction }),
  deleteTemplate: () => invoke("delete_macos_template"),
  createCheckpoint: (id, name) => invoke("create_macos_checkpoint", { id, name }),
  restoreCheckpoint: (id, checkpointId) => invoke("restore_macos_checkpoint", { id, checkpointId }),
  forkCheckpoint: (id, checkpointId, newName) => invoke("fork_macos_checkpoint", { id, checkpointId, newName }),
  deleteCheckpoint: (id, checkpointId) => invoke("delete_macos_checkpoint", { id, checkpointId }),
  remote: {
    snapshot: deviceId => invoke("remote_macos_snapshot", { deviceId }),
    create: (deviceId, request) => invoke("remote_macos_create", { deviceId, request }),
    action: (deviceId, computerId, action) => invoke("remote_macos_action", { deviceId, computerId, action }),
    openDisplay: (deviceId, computerId) => invoke("open_macos_remote_display", { deviceId, computerId }),
  },
  listen: handler => listen("silo://macos-computers-changed", event => handler(event.payload)),
}

export const nativeMacosComputersStore = createMacosComputersStore(nativeMacosComputersBackend)

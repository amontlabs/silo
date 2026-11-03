import { createContext, useContext } from "react"
import { invoke } from "@tauri-apps/api/core"
import { parseLinuxDesktopState, type ComputerUseApproval, type LinuxDesktopState } from "./linux-desktop-state"

/** What built-in computer use needs from its host: the native commands in production,
 * deterministic fixtures in the browser preview. */
export interface ComputerUseBackend {
  readDesktopState(computer: string): Promise<unknown>
  setApproval(computer: string, mode: ComputerUseApproval): Promise<unknown>
  setup(computer: string): Promise<unknown>
}

export const nativeComputerUseBackend: ComputerUseBackend = {
  readDesktopState: computer => invoke("read_desktop_state", { computer }),
  setApproval: (computer, mode) => invoke("set_computer_use_approval", { computer, mode }),
  setup: computer => invoke("desktop_action", { computer, action: "setup-computer-use" }),
}

export interface ComputerUseBridge {
  readState(computer: string): Promise<LinuxDesktopState>
  setApproval(computer: string, mode: ComputerUseApproval): Promise<LinuxDesktopState>
  setup(computer: string): Promise<LinuxDesktopState>
}

export function createComputerUseBridge(backend: ComputerUseBackend): ComputerUseBridge {
  return {
    readState: async computer => parseLinuxDesktopState(await backend.readDesktopState(computer)),
    setApproval: async (computer, mode) => parseLinuxDesktopState(await backend.setApproval(computer, mode)),
    setup: async computer => parseLinuxDesktopState(await backend.setup(computer)),
  }
}

export const ComputerUseContext = createContext<ComputerUseBridge | null>(null)

export function useComputerUseBridge() { return useContext(ComputerUseContext) }

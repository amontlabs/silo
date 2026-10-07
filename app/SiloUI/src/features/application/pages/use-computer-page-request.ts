import { useEffect, useEffectEvent, useRef, useState } from "react"

import type { ApplicationComputer, ApplicationSource, ComputerDetailTab } from "@/features/application/model/application-source"
import { computerAvailability } from "../model/computer-availability"
import type { ComputerCommandRequest } from "../components/application-commands"

/** A command palette request carried out on a computer's page. */
export interface ComputerPageRequest {
  token: number
  computerId: string
  request: ComputerCommandRequest
}

/** Carries out palette requests on a computer's page: its folder picker, or the ⋯ popover its own controls open. */
export function useComputerPageRequest({ active, selectedId, activeComputerTab, computerRequest, onComputerRequestHandled, openComputer, computers, source }: {
  active: boolean
  selectedId: string | null
  activeComputerTab: ComputerDetailTab
  computerRequest?: ComputerPageRequest
  onComputerRequestHandled?: (token: number) => void
  openComputer: (computerId: string) => void
  computers: ReadonlyMap<string, ApplicationComputer>
  source: ApplicationSource
}) {
  // The editor folder picker replaces the page for the route it was opened from. It closes
  // for good when that route changes (palette, status panel, Back/Forward, another section)
  // or when its computer can no longer be opened, so it never takes the screen over later.
  const [folderPicker, setFolderPicker] = useState<{ computerId: string; route: string } | null>(null)
  const pickerRoute = `${active}:${selectedId ?? ""}:${activeComputerTab}`
  const openFolderPicker = (computerId: string) => setFolderPicker({ computerId, route: pickerRoute })
  // Palette requests open the computer's page (the app navigates there first) and then the
  // same folder picker or ⋯ popover its own controls open.
  const [menuRequest, setMenuRequest] = useState<{ token: number; computerId: string; panel: string }>()
  const handledComputerRequest = useRef(0)
  const runComputerRequest = useEffectEvent((request: ComputerPageRequest) => {
    openComputer(request.computerId)
    if (request.request === "editor") setFolderPicker({ computerId: request.computerId, route: `${active}:${request.computerId}:${activeComputerTab}` })
    else setMenuRequest({ token: request.token, computerId: request.computerId, panel: request.request })
    onComputerRequestHandled?.(request.token)
  })
  useEffect(() => {
    if (!computerRequest || handledComputerRequest.current === computerRequest.token) return
    handledComputerRequest.current = computerRequest.token
    runComputerRequest(computerRequest)
  }, [computerRequest])
  // The request belongs to the page it was made for: leaving that page drops it, so the
  // popover never reopens when the page is shown again later.
  if (menuRequest && selectedId !== null && selectedId !== menuRequest.computerId) setMenuRequest(undefined)
  if (menuRequest && selectedId === null && !computerRequest) setMenuRequest(undefined)
  const folderComputer = folderPicker && folderPicker.route === pickerRoute ? computers.get(folderPicker.computerId) : undefined
  const showFolderPicker = Boolean(folderComputer && computerAvailability(folderComputer, source).canOpen)
  // Adjusting state while rendering: the picker is dropped before it could reappear.
  if (folderPicker && !showFolderPicker) setFolderPicker(null)
  return { folderComputer, showFolderPicker, openFolderPicker, closeFolderPicker: () => setFolderPicker(null), menuRequest }
}

import { createContext, useContext } from "react"

import type { SetupComputerConfiguration } from "@/contracts/silo"
import type { ComputerReview } from "./computer-review"
import type { ComputerEditorDraft } from "@/features/onboarding/model/onboarding-draft"

/**
 * Open computer editors, kept in memory while the application window is mounted, so
 * navigating away (⌘1–7, ⌘[, the breadcrumb) and back restores the unsaved edit instead of
 * discarding it. Each editor surface uses its own key (the list, or one computer's page).
 * Save, Cancel and Discard clear the entry. Onboarding persists its editor itself
 * (`onEditorDraftChange`); without a `ComputerEditorDraftsProvider` nothing is kept.
 */
export interface StoredComputerEditor {
  editor: ComputerEditorDraft
  /** The edited computer's saved configuration when the editor opened. */
  editorBaseline: SetupComputerConfiguration | null
  /** Every computer's saved configuration when the edit began (the change's `expected`). */
  baseline: SetupComputerConfiguration[] | null
  deviceId: string
  editorConflict?: boolean
  editorReview?: ComputerReview | null
  /** A save that must stay locked and settle even if its editor surface unmounts. */
  pendingSave?: Promise<void>
  macosForm?: MacosEditorState
  /** A macOS creation started from this editor; a restored editor observes it instead of starting another. */
  pendingMacosCreate?: Promise<void>
}

/** The macOS side of a new-computer editor: the chosen operating system and the macOS fields. */
export interface MacosEditorState {
  /** The id of the editor draft this belongs to. */
  editorId: string
  os: "linux" | "macos"
  name: string
  cpus: string
  memoryGiB: string
  diskGiB: string
  /** The device that will host the computer; "" or absent is this device. */
  deviceId?: string
  /** A macOS creation started from this editor has not finished. */
  creating: boolean
}

export const ComputerEditorDraftsContext = createContext<Map<string, StoredComputerEditor> | null>(null)

export function useComputerEditorDrafts() {
  return useContext(ComputerEditorDraftsContext)
}

import { createElement, useEffect, useRef, useState } from "react"

import { useComputerUseBridge } from "@/desktop/computer-use-bridge"
import { CreatedComputerApprovalSwitch } from "@/desktop/created-computer-toast"
import { dismissOperationToast, showOperationFailure, showOperationProgress, showOperationSuccess } from "@/lib/operation-toast"

import type { ApplicationComputer, ComputerConfigurationOperation } from "@/features/application/model/application-source"
import { computerUsePending, describeConfiguration } from "@/features/application/model/computer-configuration-progress"
import { computerTarget } from "@/features/application/model/connections"

const TOAST_ID = "computer-configuration"
/** A settings change that finishes within this time never flashes a notification. */
const SAVE_DEBOUNCE_MS = 500

/**
 * Drives one notification for computer configuration changes wherever the user is: a progress
 * toast with the current step while it applies, then "Created {name}" with the computer use
 * approval switch (or a failure). Renders nothing itself.
 */
export function ComputerConfigurationToast({ operation, computers, onOpen }: {
  operation: ComputerConfigurationOperation | null
  computers: readonly ApplicationComputer[]
  onOpen?: (computerId: string) => void
}) {
  const bridge = useComputerUseBridge()
  const local = new Map(computers.filter(computer => !computer.device).map(computer => [computer.configuration.id, computer.configuration.name]))
  const description = operation ? describeConfiguration(operation, local) : null
  const applying = operation?.status === "applying"
  const [debounced, setDebounced] = useState(false)
  const show = applying && (description?.kind !== "saving" || debounced)
  const tracked = useRef<{ creating: string[]; startedAt: number; shown: boolean; pending: boolean; desktop: boolean } | null>(null)
  const latest = useRef({ computers, bridge, onOpen })
  latest.current = { computers, bridge, onOpen }

  useEffect(() => {
    if (!applying) { setDebounced(false); return }
    const timer = window.setTimeout(() => setDebounced(true), SAVE_DEBOUNCE_MS)
    return () => window.clearTimeout(timer)
  }, [applying, operation?.id])

  useEffect(() => {
    if (!operation || !description) return
    if (operation.status === "applying") {
      tracked.current ??= { creating: [], startedAt: Date.now(), shown: false, pending: false, desktop: false }
      tracked.current.creating = description.creating
      tracked.current.pending = computerUsePending(operation)
      // The backend adds the built-in desktop to a new computer on a v4 image, so neither the request nor a lagging
      // snapshot may carry it. The request and the setup steps it reports are what is known while it is created.
      tracked.current.desktop ||= description.creating.length === 1 && (operation.candidate.computers.some(configuration => configuration.name === description.creating[0] && Boolean(configuration.desktop))
        || operation.progressEvents.some(event => event.step === "desktop-installation" || event.step === "computer-use-setup" || event.step === "computer-use-pending"))
      if (!show) return
      tracked.current.shown = true
      showOperationProgress(TOAST_ID, {
        title: description.title,
        step: description.step,
        progress: description.progress,
        startedAt: tracked.current.startedAt,
        computer: description.creating,
      })
    } else if (operation.status === "failed" && tracked.current?.shown) {
      tracked.current = null
      showOperationFailure(TOAST_ID, description.kind === "creating" ? `Could not create ${description.creating[0]}` : "Computer changes failed", { description: operation.error.message, native: false })
    }
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [operation, show])

  // The operation ends by disappearing: a success for what it created, or nothing when it
  // only changed or removed computers.
  const wasApplying = useRef(false)
  useEffect(() => {
    const ended = wasApplying.current && !operation
    wasApplying.current = applying
    if (!ended) return
    const finished = tracked.current
    tracked.current = null
    if (!finished?.shown) return
    const { computers: current, bridge: computerUse, onOpen: open } = latest.current
    const created = finished.creating.flatMap(name => current.filter(computer => !computer.device && computer.configuration.name === name))
    if (created.length === 0) { dismissOperationToast(TOAST_ID); return }
    const [first] = created
    const single = created.length === 1
    showOperationSuccess(TOAST_ID, single ? `Created ${first.configuration.name}` : `Created ${created.length} computers`, {
      persist: true,
      computer: created.map(computer => computer.configuration.name),
      description: single && computerUse && (first.configuration.desktop || finished.desktop)
        ? createElement("div", { className: "grid gap-1.5" },
          finished.pending ? createElement("p", { className: "text-xs text-muted-foreground" }, "Computer use will finish setting up at first start.") : null,
          createElement(CreatedComputerApprovalSwitch, { computer: computerTarget(first) }))
        : undefined,
      action: single && open ? { label: "Open", onClick: () => open(first.configuration.id) } : undefined,
    })
  }, [operation, applying])

  useEffect(() => () => { dismissOperationToast(TOAST_ID) }, [])
  return null
}

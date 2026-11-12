import { useCallback, useEffect, useState } from "react"

import type { ApplicationTab, ComputerDetailTab, SettingsSection, ComputerSection } from "./application-source"

interface Location {
  tab: ApplicationTab
  computerSection: ComputerSection
  settingsSection: SettingsSection
  /** The computer whose detail page is open in the Computers overview, by configuration id. */
  computer?: string
  /** The active tab on that computer's detail page. */
  computerTab?: ComputerDetailTab
}

export type ApplicationInitialRoute = Partial<Location> & { computer?: string; /** With the Pending tab: the approval to open. */ approval?: string }

interface History {
  entries: Location[]
  index: number
}

function sameDestination(left: Location, right: Location) {
  if (left.tab !== right.tab) return false
  if (left.tab === "settings") return left.settingsSection === right.settingsSection
  if (left.tab !== "computers") return true
  if (left.computerSection !== right.computerSection) return false
  // On the Computers overview an open computer detail (and its tab) is its own destination.
  if (left.computerSection !== "overview") return true
  return (left.computer ?? undefined) === (right.computer ?? undefined)
    && (left.computerTab ?? undefined) === (right.computerTab ?? undefined)
}

function withoutSystemIssue(history: History): History {
  if (!history.entries.some(({ tab }) => tab === "system")) return history
  const entries: Location[] = []
  let index = 0
  history.entries.forEach((entry, position) => {
    if (entry.tab === "system" && position !== history.index) return
    const destination: Location = entry.tab === "system"
      ? { ...entry, tab: "computers", computerSection: "overview" }
      : entry
    if (!entries.length || !sameDestination(entries.at(-1)!, destination)) entries.push(destination)
    if (position <= history.index) index = entries.length - 1
  })
  return { entries, index }
}

export function useApplicationNavigation(hasSystemIssue: boolean, initialRoute?: ApplicationInitialRoute) {
  const [storedHistory, setHistory] = useState<History>({
    entries: [{
      tab: initialRoute?.tab ?? "computers",
      computerSection: initialRoute?.computerSection ?? "overview",
      settingsSection: initialRoute?.settingsSection ?? "general",
      // A deep link into the overview can preselect a computer detail page and tab.
      ...((initialRoute?.computerSection ?? "overview") === "overview" && initialRoute?.computer
        ? { computer: initialRoute.computer, computerTab: initialRoute.computerTab ?? "overview" }
        : {}),
    }],
    index: 0,
  })
  const history = hasSystemIssue ? storedHistory : withoutSystemIssue(storedHistory)

  useEffect(() => {
    // A resolved issue is removed from history as well as the sidebar.
    // oxlint-disable-next-line react/set-state-in-effect
    if (!hasSystemIssue) setHistory(withoutSystemIssue)
  }, [hasSystemIssue])

  const navigate = useCallback((change: Partial<Location>) => {
    setHistory((stored) => {
      const current = hasSystemIssue ? stored : withoutSystemIssue(stored)
      const location = current.entries[current.index]
      const next = { ...location, ...change }
      if (sameDestination(location, next)) return current
      return { entries: [...current.entries.slice(0, current.index + 1), next], index: current.index + 1 }
    })
  }, [hasSystemIssue])

  // A computer that no longer exists (deleted, or gone after a refresh) turns every entry
  // that showed it into the plain Computers list, in place: the current position and the
  // forward history survive, and Back never lands on (and bounces off) a missing page.
  const forgetComputers = useCallback((exists: (computer: string) => boolean) => {
    setHistory((stored) => {
      if (!stored.entries.some(({ computer }) => computer !== undefined && !exists(computer))) return stored
      const entries: Location[] = []
      let index = 0
      stored.entries.forEach((entry, position) => {
        const destination: Location = entry.computer !== undefined && !exists(entry.computer)
          ? { ...entry, computer: undefined, computerTab: undefined }
          : entry
        if (!entries.length || !sameDestination(entries.at(-1)!, destination)) entries.push(destination)
        if (position <= stored.index) index = entries.length - 1
      })
      return { entries, index }
    })
  }, [])

  const move = useCallback((offset: number) => {
    setHistory((stored) => {
      const current = hasSystemIssue ? stored : withoutSystemIssue(stored)
      const index = Math.max(0, Math.min(current.index + offset, current.entries.length - 1))
      return index === current.index ? current : { ...current, index }
    })
  }, [hasSystemIssue])

  return {
    ...history.entries[history.index],
    canGoBack: history.index > 0,
    canGoForward: history.index < history.entries.length - 1,
    goBack: () => move(-1),
    goForward: () => move(1),
    selectTab: (tab: ApplicationTab) => navigate({ tab }),
    // Selecting a computer section clears any open computer detail so the section shows plainly.
    selectComputerSection: (computerSection: ComputerSection) => navigate({ tab: "computers", computerSection, computer: undefined, computerTab: undefined }),
    selectSettingsSection: (settingsSection: SettingsSection) => navigate({ tab: "settings", settingsSection }),
    openComputer: (computer: string, computerTab: ComputerDetailTab = "overview") => navigate({ tab: "computers", computerSection: "overview", computer, computerTab }),
    selectComputerTab: (computerTab: ComputerDetailTab) => navigate({ tab: "computers", computerSection: "overview", computerTab }),
    closeComputer: () => navigate({ tab: "computers", computerSection: "overview", computer: undefined, computerTab: undefined }),
    forgetComputers,
  }
}

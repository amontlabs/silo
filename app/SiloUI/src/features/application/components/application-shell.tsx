import { useEffect, useEffectEvent, useRef, useState, type ReactNode } from "react"
import { Activity, Bell, Boxes, ChevronRight, CircleAlert, File, GitFork, KeyRound, LayoutDashboard, Monitor, Network, Settings2, SlidersHorizontal, Terminal } from "lucide-react"

import { ShortcutBadge } from "@/components/shortcut-badge"
import { shortcutFor, type KeyboardShortcut } from "@/lib/shortcuts"
import { SiloMark } from "@/components/silo-mark"
import { SiloWindow } from "@/components/silo-window"
import { Toaster } from "@/components/ui/sonner"
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip"
import { Spinner } from "@/components/ui/spinner"
import { ApplicationTitleBar } from "@/features/application/components/application-title-bar"
import { useSidebarDisclosure } from "@/hooks/use-sidebar-disclosure"
import type { ActiveRuntimeRepairPresentation, ApplicationTab, SettingsSection, ComputerSection } from "@/features/application/model/application-source"
import { sidebarItemActiveClass, sidebarItemClass, sidebarItemLevels } from "@/components/sidebar-item"
import { cn } from "@/lib/utils"
import "./application-shell.css"

export interface ApplicationNavigationLoading {
  tabs?: Partial<Record<ApplicationTab, boolean>>
  computerSections?: Partial<Record<ComputerSection, boolean>>
  settingsSections?: Partial<Record<SettingsSection, boolean>>
}

const primaryItems = [
  { id: "github", label: "GitHub", icon: GitFork },
  { id: "secrets", label: "Secrets", icon: KeyRound },
] as const

const computerItems = [
  { id: "overview", label: "All computers", icon: LayoutDashboard },
  { id: "files", label: "Files", icon: File },
  { id: "logs", label: "Logs", icon: Terminal },
  { id: "network", label: "Network", icon: Network },
  { id: "activity", label: "Activity", icon: Activity },
] as const

const settingsItems = [
  { id: "general", label: "General", icon: SlidersHorizontal },
  { id: "connections", label: "Connections", icon: Monitor },
  { id: "notifications", label: "Notifications", icon: Bell },
] as const

function NavigationLoadingIndicator({ loading, collapsed }: { loading: boolean; collapsed: boolean }) {
  if (!loading) return null
  const spinner = <Spinner data-navigation-loading-indicator className={collapsed ? "absolute -top-1 -right-1 size-2 rounded-full bg-sidebar ring-2 ring-sidebar" : undefined} />
  return collapsed ? spinner : <span className="grid size-5 shrink-0 place-items-center">{spinner}</span>
}

/** Computers with an error or a warning; both counts are computers, never individual errors. */
export interface SidebarAttention { errors: number; warnings: number }

const countLabel = (count: number, one: string, many: string) => `${count} ${count === 1 ? one : many}`
const errorsLabel = (count: number) => countLabel(count, "computer has an error", "computers have errors")
const warningsLabel = (count: number) => countLabel(count, "computer has a warning", "computers have warnings")
const attentionLabel = ({ errors, warnings }: SidebarAttention) => countLabel(errors + warnings, "computer needs attention", "computers need attention")

/** The visual part of a collapsed menu's attention signal; its text is announced separately. */
function AttentionMark({ attention, collapsed }: { attention: SidebarAttention; collapsed: boolean }) {
  const error = attention.errors > 0
  return collapsed
    ? <span data-navigation-attention aria-hidden="true" className={cn("absolute top-1 right-1 size-1.5 rounded-full", error ? "bg-destructive" : "bg-warning")} />
    : <span data-navigation-attention aria-hidden="true" className={cn(
      "inline-flex h-5 min-w-5 shrink-0 items-center justify-center rounded-full border px-1 text-caption leading-none font-semibold tabular-nums",
      error ? "border-destructive/20 bg-destructive/10 text-destructive" : "border-warning/20 bg-warning/10 text-warning",
    )}>{attention.errors + attention.warnings}</span>
}

function hasAttention(attention?: SidebarAttention | null): attention is SidebarAttention {
  return Boolean(attention && (attention.errors > 0 || attention.warnings > 0))
}

function NavigationTooltip({ label, collapsed, children, shortcut }: { label: string; collapsed: boolean; children: ReactNode; shortcut?: KeyboardShortcut }) {
  // Hidden content still needs Radix's dismissal handlers to clear its open state.
  return <Tooltip>
    <TooltipTrigger asChild>{children}</TooltipTrigger>
    <TooltipContent side="right" hidden={!collapsed} shortcut={shortcut}>{label}</TooltipContent>
  </Tooltip>
}

function SidebarShortcut({ action }: { action: string }) {
  const shortcut = shortcutFor(action)
  return shortcut ? <ShortcutBadge shortcut={shortcut} className="pointer-events-none opacity-0 group-hover/sidebar-item:opacity-100 group-focus-visible/sidebar-item:opacity-100" /> : null
}

function NavigationButton({
  id,
  label,
  icon: Icon,
  active,
  tone = "default",
  loading = false,
  reserveDisclosure = false,
  attention,
  describedBy,
  collapsed,
  onClick,
}: {
  id: ApplicationTab
  label: string
  icon: typeof Boxes
  active: boolean
  tone?: "default" | "danger" | "warning"
  loading?: boolean
  reserveDisclosure?: boolean
  /** Attention mirrored from a collapsed menu's sections. */
  attention?: SidebarAttention | null
  describedBy?: string
  collapsed: boolean
  onClick: () => void
}) {
  return (
    <NavigationTooltip label={label} collapsed={collapsed} shortcut={shortcutFor(id === "computers" ? "go-computers" : id === "settings" ? "settings" : `go-${id}`)}>
    <button
      id={`application-nav-${id}`}
      type="button"
      data-navigation-level="primary"
      data-navigation-tone={tone}
      aria-current={active ? "page" : undefined}
      aria-controls={`application-panel-${id}`}
      aria-keyshortcuts={shortcutFor(id === "computers" ? "go-computers" : id === "settings" ? "settings" : `go-${id}`)?.aria}
      aria-busy={loading || undefined}
      aria-describedby={describedBy}
      onClick={onClick}
      className={cn(
        sidebarItemClass,
        sidebarItemLevels.primary,
        tone === "danger"
          ? "text-destructive hover:bg-destructive/10 hover:text-destructive"
          : tone === "warning"
            ? "text-warning hover:bg-warning/10 hover:text-warning"
            : undefined,
        active && (tone === "danger"
          ? "bg-destructive/10 font-medium text-destructive"
          : tone === "warning"
            ? "bg-warning/10 font-medium text-warning"
            : sidebarItemActiveClass),
        reserveDisclosure && "sidebar-primary-with-disclosure",
      )}
    >
      <span className="relative flex shrink-0">
        <Icon aria-hidden="true" className="size-4" />
        {collapsed && <NavigationLoadingIndicator loading={loading} collapsed />}
      </span>
      <span className="sidebar-label flex-1 text-left">{label}</span>
      {hasAttention(attention) && <AttentionMark attention={attention} collapsed={collapsed} />}
      {!collapsed && <NavigationLoadingIndicator loading={loading} collapsed={false} />}
      {!collapsed && <SidebarShortcut action={id === "computers" ? "go-computers" : id === "settings" ? "settings" : `go-${id}`} />}
    </button>
    </NavigationTooltip>
  )
}

function DisclosureNavigationItem({
  id,
  label,
  icon,
  active,
  expanded,
  collapsed,
  attention,
  loading = false,
  onSelect,
  onToggle,
  children,
}: {
  id: "computers" | "settings"
  label: string
  icon: typeof Boxes
  active: boolean
  expanded: boolean
  collapsed: boolean
  /** Attention reported by the sections; shown on this item while its menu is closed. */
  attention?: SidebarAttention | null
  /** Whether any section has work in progress; shown on this item while its menu is closed. */
  loading?: boolean
  onSelect: () => void
  onToggle: () => void
  children: ReactNode
}) {
  const menuID = `${id}-sections`
  const attentionID = `${id}-attention`
  const mirroredAttention = !expanded && hasAttention(attention) ? attention : null

  return (
    <div className="grid w-full grid-cols-1 gap-1">
      <div className="group/sidebar-item relative w-full">
        <NavigationButton id={id} label={label} icon={icon} active={active} collapsed={collapsed} reserveDisclosure attention={mirroredAttention} describedBy={mirroredAttention ? attentionID : undefined} loading={!expanded && loading} onClick={onSelect} />
        {mirroredAttention && <span id={attentionID} role="status" aria-label={attentionLabel(mirroredAttention)} className="sr-only">{attentionLabel(mirroredAttention)}</span>}
        <button
          type="button"
          aria-label={`${expanded ? "Collapse" : "Expand"} ${label} menu`}
          aria-expanded={expanded}
          aria-controls={menuID}
          aria-hidden={collapsed || undefined}
          tabIndex={collapsed ? -1 : undefined}
          onClick={onToggle}
          className="sidebar-disclosure absolute top-1 right-1 z-10 grid size-8 place-items-center rounded-md text-foreground/65 hover:bg-sidebar-accent hover:text-foreground focus-ring"
        >
          <ChevronRight className={cn("size-4 transition-transform", expanded && "rotate-90")} />
        </button>
      </div>
      {expanded && <div id={menuID}>{children}</div>}
    </div>
  )
}

function SubNavigation<Section extends string>({
  label,
  items,
  section,
  active,
  attention,
  loading,
  collapsed,
  onSelect,
}: {
  label: string
  items: ReadonlyArray<{ id: Section; label: string; icon: typeof Boxes }>
  section: Section
  active: boolean
  attention?: SidebarAttention & { section: Section } | null
  loading?: Partial<Record<Section, boolean>>
  collapsed: boolean
  onSelect: (section: Section) => void
}) {
  return (
    <div role="group" aria-label={label} className="sidebar-subnav relative grid grid-cols-1 gap-1">
      {items.map(({ id, label: itemLabel, icon: Icon }) => (
        <NavigationTooltip key={id} label={itemLabel} collapsed={collapsed} shortcut={shortcutFor(id === "overview" ? "go-computers" : id === "general" ? "settings" : `go-${id}`)}>
        <button
          type="button"
          aria-current={active && section === id ? "page" : undefined}
          aria-busy={loading?.[id] || undefined}
          aria-keyshortcuts={shortcutFor(id === "overview" ? "go-computers" : id === "general" ? "settings" : `go-${id}`)?.aria}
          onClick={() => onSelect(id)}
          className={cn(
            sidebarItemClass,
            sidebarItemLevels.secondary,
            active && section === id && sidebarItemActiveClass,
          )}
        >
          <span className="relative flex shrink-0">
            <Icon aria-hidden="true" className="sidebar-section-icon" />
            {collapsed && <NavigationLoadingIndicator loading={loading?.[id] ?? false} collapsed />}
          </span>
          <span className="sidebar-label flex-1 text-left">{itemLabel}</span>
          {collapsed && attention?.section === id && <span
            role="status"
            aria-label={attentionLabel(attention)}
            className={cn("absolute top-1 right-1 size-1.5 rounded-full", attention.errors > 0 ? "bg-destructive" : "bg-warning")}
          />}
          {!collapsed && attention?.section === id && (
            <span className="flex shrink-0 items-center gap-1">
              <TooltipProvider delayDuration={150}>
                {attention.errors > 0 && (
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <span
                        role="status"
                        aria-label={errorsLabel(attention.errors)}
                        className="inline-flex h-5 min-w-5 items-center justify-center rounded-full border border-destructive/20 bg-destructive/10 px-1 text-caption leading-none font-semibold tabular-nums text-destructive"
                      >
                        {attention.errors}
                      </span>
                    </TooltipTrigger>
                    <TooltipContent>{errorsLabel(attention.errors)}</TooltipContent>
                  </Tooltip>
                )}
                {attention.warnings > 0 && (
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <span
                        role="status"
                        aria-label={warningsLabel(attention.warnings)}
                        className="inline-flex h-5 min-w-5 items-center justify-center rounded-full border border-warning/20 bg-warning/10 px-1 text-caption leading-none font-semibold tabular-nums text-warning"
                      >
                        {attention.warnings}
                      </span>
                    </TooltipTrigger>
                    <TooltipContent>{warningsLabel(attention.warnings)}</TooltipContent>
                  </Tooltip>
                )}
              </TooltipProvider>
              </span>
            )}
          {!collapsed && <NavigationLoadingIndicator loading={loading?.[id] ?? false} collapsed={false} />}
          {!collapsed && <SidebarShortcut action={id === "overview" ? "go-computers" : id === "general" ? "settings" : `go-${id}`} />}
        </button>
        </NavigationTooltip>
      ))}
    </div>
  )
}

export function ApplicationShell({
  activeTab,
  computerSection,
  settingsSection,
  systemIssueStatus,
  computerAttention,
  navigationLoading,
  navigationDisabled = false,
  defaultSettingsMenuOpen = activeTab === "settings",
  toggleSidebarRequest,
  onSidebarCollapsedChange,
  onTabChange,
  onComputerSectionChange,
  onSettingsSectionChange,
  canGoBack,
  canGoForward,
  onGoBack,
  onGoForward,
  reduceMotion = false,
  commandMenu,
  notice,
  banner,
  children,
}: {
  activeTab: ApplicationTab
  computerSection: ComputerSection
  settingsSection: SettingsSection
  systemIssueStatus: ActiveRuntimeRepairPresentation["status"] | null
  computerAttention: SidebarAttention
  navigationLoading?: ApplicationNavigationLoading
  navigationDisabled?: boolean
  defaultSettingsMenuOpen?: boolean
  toggleSidebarRequest?: number
  onSidebarCollapsedChange?: (collapsed: boolean) => void
  onTabChange: (tab: ApplicationTab) => void
  onComputerSectionChange: (section: ComputerSection) => void
  onSettingsSectionChange: (section: SettingsSection) => void
  canGoBack: boolean
  canGoForward: boolean
  onGoBack: () => void
  onGoForward: () => void
  reduceMotion?: boolean
  commandMenu?: ReactNode
  notice?: ReactNode
  /** In-flow content above every page; unlike `notice`, it never covers the page. */
  banner?: ReactNode
  children: ReactNode
}) {
  const [computerMenuOpen, setComputerMenuOpen] = useState(activeTab === "computers")
  const [settingsMenuOpen, setSettingsMenuOpen] = useState(defaultSettingsMenuOpen)
  const {
    collapsed: pinnedCollapsed,
    previewing,
    sidebarRef,
    toggleRef,
    toggle,
    enterToggle,
    leaveToggle,
    enterSidebar,
    leaveSidebar,
    usePointer,
    useKeyboard,
    blurSidebar,
  } = useSidebarDisclosure()
  useEffect(() => { onSidebarCollapsedChange?.(pinnedCollapsed) }, [pinnedCollapsed, onSidebarCollapsedChange])
  const consumedToggleRequest = useRef(0)
  const toggleRequested = useEffectEvent(() => { if (!navigationDisabled) toggle() })
  useEffect(() => {
    if (!toggleSidebarRequest || consumedToggleRequest.current === toggleSidebarRequest) return
    consumedToggleRequest.current = toggleSidebarRequest
    toggleRequested()
  }, [toggleSidebarRequest])
  const collapsed = pinnedCollapsed && !previewing

  function selectTab(tab: ApplicationTab) {
    if (tab === "computers") setComputerMenuOpen(true)
    if (tab === "settings") setSettingsMenuOpen(true)
    onTabChange(tab)
  }

  return (
    <TooltipProvider delayDuration={300} reduceMotion={reduceMotion}>
    <SiloWindow title="Silo" label="Silo" reduceMotion={reduceMotion} className={cn("silo-application", pinnedCollapsed && "sidebar-pinned-collapsed")} titleBar={
      <ApplicationTitleBar disabled={navigationDisabled} collapsed={pinnedCollapsed} previewing={previewing} toggleRef={toggleRef} onToggleSidebar={toggle} onPreviewEnter={enterToggle} onPreviewLeave={leaveToggle} canGoBack={canGoBack} canGoForward={canGoForward} onGoBack={onGoBack} onGoForward={onGoForward} commandMenu={commandMenu} />
    }>
      <div className="sidebar-layout grid min-h-0 flex-1" data-sidebar-layout={pinnedCollapsed ? "collapsed" : "expanded"}>
        <nav
          ref={sidebarRef}
          id="application-sidebar"
          aria-label="Silo navigation"
          inert={navigationDisabled || undefined}
          data-collapsed={collapsed}
          data-previewing={previewing}
          onPointerEnter={enterSidebar}
          onPointerLeave={leaveSidebar}
          onPointerDown={usePointer}
          onKeyDown={useKeyboard}
          onBlurCapture={blurSidebar}
          className="silo-sidebar flex min-h-0 min-w-0 flex-col overflow-x-hidden overflow-y-auto border-r border-border bg-sidebar py-4"
        >
          <div className="sidebar-brand mb-2 flex shrink-0 items-center gap-3 overflow-hidden pb-2">
            <SiloMark className="size-8 shrink-0" />
            <span className="sidebar-label text-base font-semibold tracking-tight">Silo</span>
          </div>
          <div className="flex w-full flex-1 flex-col items-start gap-1">
            <div className="grid w-full grid-cols-1 gap-1">
              <DisclosureNavigationItem
                id="computers"
                label="Computers"
                icon={Boxes}
                active={activeTab === "computers"}
                expanded={computerMenuOpen}
                collapsed={collapsed}
                attention={computerAttention}
                loading={Object.values(navigationLoading?.computerSections ?? {}).some(Boolean)}
                onSelect={() => selectTab("computers")}
                onToggle={() => setComputerMenuOpen((open) => !open)}
              >
                <SubNavigation
                  label="Computer sections"
                  items={computerItems}
                  section={computerSection}
                  active={activeTab === "computers"}
                  collapsed={collapsed}
                  attention={hasAttention(computerAttention) ? { section: "overview", ...computerAttention } : null}
                  loading={navigationLoading?.computerSections}
                  onSelect={(section) => {
                    onComputerSectionChange(section)
                    setComputerMenuOpen(true)
                  }}
                />
              </DisclosureNavigationItem>
              {primaryItems.map(({ id, label, icon }) => (
                <NavigationButton key={id} id={id} label={label} icon={icon} active={activeTab === id} collapsed={collapsed} loading={navigationLoading?.tabs?.[id]} onClick={() => selectTab(id)} />
              ))}
            </div>
            <div className="mt-auto grid w-full grid-cols-1 gap-1">
              <div className="sidebar-footer-divider my-2 border-t border-border" aria-hidden="true" />
              {systemIssueStatus && (
                <NavigationButton
                  id="system"
                  label="System issue"
                  icon={CircleAlert}
                  loading={navigationLoading?.tabs?.system}
                  active={activeTab === "system"}
                  collapsed={collapsed}
                  tone="danger"
                  onClick={() => selectTab("system")}
                />
              )}
              <DisclosureNavigationItem
                id="settings"
                label="Settings"
                icon={Settings2}
                active={activeTab === "settings"}
                expanded={settingsMenuOpen}
                collapsed={collapsed}
                loading={Object.values(navigationLoading?.settingsSections ?? {}).some(Boolean)}
                onSelect={() => selectTab("settings")}
                onToggle={() => setSettingsMenuOpen((open) => !open)}
              >
                <SubNavigation
                  label="Settings sections"
                  items={settingsItems}
                  section={settingsSection}
                  active={activeTab === "settings"}
                  collapsed={collapsed}
                  loading={navigationLoading?.settingsSections}
                  onSelect={(section) => {
                    onSettingsSectionChange(section)
                    setSettingsMenuOpen(true)
                  }}
                />
              </DisclosureNavigationItem>
            </div>
          </div>
        </nav>
        <div className="application-content relative flex min-h-0 min-w-0 flex-col">
          <div className="pointer-events-none absolute inset-x-0 top-3 z-20 mx-auto flex w-full max-w-4xl justify-center px-4 sm:px-6">{notice}</div>
          {banner}
          <div inert={navigationDisabled || undefined} className={cn("min-h-0 min-w-0 flex-1", activeTab === "computers" ? "overflow-hidden" : "overflow-y-auto")}>{children}</div>
        </div>
      </div>
      <Toaster reduceMotion={reduceMotion} />
    </SiloWindow>
    </TooltipProvider>
  )
}

import type { LogWindow } from "./logs-page"
import { visibleText } from "@/lib/visible-text"
import { computerTarget } from "@/features/application/model/connections"
import { FolderActions } from "@/features/application/components/folder-actions"
import { ComputerFileTree } from "@/features/application/components/computer-file-tree"
import { useFileTransferControls } from "@/features/application/components/use-file-transfers"
import type { createDirectoryStore } from "@/features/application/model/directory-store"
import { memo, Suspense, useMemo, useState } from "react"
import { Activity, Archive, Box, Boxes, Check, CircleAlert, Cloud, File, GitBranch, KeyRound, Plus, RefreshCw, TriangleAlert, Wrench } from "lucide-react"

import { DisclosureHeader } from "@/components/disclosure-header"
import { EmptyState } from "@/components/empty-state"
import { ErrorDetails } from "@/components/error-details"
import { FilterCombobox, type FilterOption } from "@/components/filter-combobox"
import { ListCard, ListRow, ListRowIcon } from "@/components/list-row"
import { StatusBadge } from "@/components/status-badge"
import { statusTones } from "@/components/status-tone"
import { Button } from "@/components/ui/button"
import { Collapsible, CollapsibleContent } from "@/components/ui/collapsible"
import { Progress } from "@/components/ui/progress"
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip"
import { Spinner } from "@/components/ui/spinner"
import { RepositoryPushButton, RepositoryPushFeedback, type PushRepository } from "@/features/application/components/repository-push-feedback"
import { useRepositoryPushToasts } from "@/features/application/components/use-repository-push-toasts"
import type { OperationQueue } from "@/features/application/model/operation-queue"
import { ComputerBadge } from "@/features/application/components/application-ui"
import type { ApplicationActions, ApplicationSource, ApplicationActivity, ApplicationActivityCategory, ApplicationComputer, RepositoryPushOperation, RepositoryPushTarget, ComputerDetailSection } from "@/features/application/model/application-source"
import { commitLabel } from "@/features/application/model/repository-push"
import { computerAvailability } from "@/features/application/model/computer-availability"
import { showActionFailure } from "@/lib/operation-toast"
import { cn } from "@/lib/utils"
import { useStableCallback } from "@/lib/use-stable-callback"
import { lazyPage } from "@/features/application/components/lazy-page"

const logsPage = lazyPage(() => import("./logs-page"), "Logs")
const networkPage = lazyPage(() => import("./network-page"), "NetworkPage")
/** Section pages loaded on demand; fetched ahead once the application is idle. */
export const computerSectionPages = [logsPage, networkPage]
const Logs = logsPage.Component
const NetworkPage = networkPage.Component

function ComputerFilterBar({
  computers,
  selectedComputerIds,
  onChange,
}: {
  computers: ApplicationComputer[]
  selectedComputerIds: ReadonlySet<string>
  onChange: (selectedComputerIds: Set<string>) => void
}) {
  return (
    <div className="border-b border-border pb-4">
      <FilterCombobox
        options={computers.map(({ configuration }) => ({ value: configuration.id, label: configuration.name }))}
        selectedValues={selectedComputerIds}
        onChange={onChange}
        label="Computer filters"
        inputLabel="Filter computers"
        className="[&_input]:h-7"
        placeholder="Filter computers…"
        listLabel="Available computer filters"
        selectedLabel="Selected computers"
        emptyMessage="No computers available."
      />
    </div>
  )
}

function Files({
  source,
  onRefreshRepositories,
  computers,
  repositoryPushOperations,
  onPushRepository,
  onDismissRepositoryPush,
  editor,
  onOpenEditor,
  directoryStore,
  active,
}: {
  source: ApplicationSource
  onRefreshRepositories?: () => Promise<void>
  editor: string
  onOpenEditor: (computer: string, path: string) => void
  directoryStore: ReturnType<typeof createDirectoryStore>
  active: boolean
  computers: ApplicationComputer[]
  repositoryPushOperations: RepositoryPushOperation[]
  onPushRepository: PushRepository
  onDismissRepositoryPush: (computer: string, repositoryPath: string) => void
}) {
  const [refreshing, setRefreshing] = useState(false)
  const refreshRepositories = async () => {
    if (!onRefreshRepositories || refreshing) return
    setRefreshing(true)
    try { await onRefreshRepositories() }
    catch (error) { showActionFailure("Could not refresh repositories", error, () => void refreshRepositories(), { native: false }) }
    finally { setRefreshing(false) }
  }
  const [repositoriesOpen, setRepositoriesOpen] = useState(true)
  const [fileTreeOpen, setFileTreeOpen] = useState(true)
  const transfers = useFileTransferControls()
  if (computers.length === 0) return <EmptyState icon={<File />} title="No matching computers" description="Clear the computer filter to browse files and repositories in every computer." />
  const repositories = computers.flatMap((computer) => computer.repositories.map((repository) => ({ computer, repository })))
  const pushOperations = new Map(repositoryPushOperations.map((operation) => [`${operation.computer}:${operation.repositoryPath}`, operation]))

  return (
    <div className="flex h-full min-h-0 flex-col justify-between gap-3 lg:grid lg:grid-cols-2 lg:grid-rows-1 lg:content-stretch lg:gap-0" data-files-layout data-file-tree-state={fileTreeOpen ? "open" : "closed"}>
      <Collapsible
        asChild
        open={repositoriesOpen}
        onOpenChange={setRepositoriesOpen}
      >
        <section
          aria-label="Repositories"
          className={cn(
            "collapsible-motion flex min-h-0 min-w-0 max-h-full flex-col overflow-hidden transition-[max-height] duration-100 ease-out lg:h-full lg:max-h-none lg:pr-5 lg:transition-none",
            repositoriesOpen ? fileTreeOpen ? "max-h-[50%] shrink-0" : "flex-1" : "max-h-8 shrink-0",
          )}
          data-files-pane="repositories"
          data-pane-position="top"
        >
          <DisclosureHeader
            className="h-8 shrink-0 px-2 py-0"
            title="Repositories"
            titleClassName="text-sm font-medium"
            label={`${repositoriesOpen ? "Collapse" : "Expand"} repositories`}
            actions={<Button variant="ghost" size="icon" className="size-6" aria-label="Refresh repositories" title="Refresh repositories" disabled={refreshing || !onRefreshRepositories} onClick={() => void refreshRepositories()}><RefreshCw aria-hidden="true" className={cn("size-3.5", refreshing && "motion-safe:animate-spin motion-reduce:animate-none")} /></Button>}
            controlsLabel="Repository pane controls"
          />
          <CollapsibleContent className="file-pane-content-motion min-h-0 flex-1" data-files-pane-content="repositories">
            <div className="h-full overflow-y-auto overscroll-contain px-2 pt-2" data-files-pane-scroll="repositories">
              {repositories.length > 0 ? (
                <ListCard divided role="list" aria-label="Repositories">
                  {repositories.map(({ computer, repository }) => {
                    const operation = pushOperations.get(`${computerTarget(computer)}:${repository.path}`)
                    const push = (target: RepositoryPushTarget) => onPushRepository(computerTarget(computer), repository.path, operation?.commitCount ?? repository.ahead, target)
                    // Same gate and name as the status bar: identical buttons need the repository and computer.
                    const canPush = computerAvailability(computer, source).canOpen
                    const label = computer.device ? `${computer.configuration.name} on ${computer.device.name}` : computer.configuration.name
                    return (
                      <div key={`${computer.configuration.id}:${repository.path}`} role="listitem" aria-busy={operation?.status === "pushing" || undefined} className="group/folder transition-colors row-hover">
                        <ListRow
                          data-repository-header
                          icon={<ListRowIcon aria-hidden="true"><GitBranch className="size-3.5" /></ListRowIcon>}
                          title={<TooltipProvider delayDuration={150}><Tooltip>
                            <TooltipTrigger asChild><span className="truncate" tabIndex={0}>{visibleText(repository.path.split("/").filter(Boolean).at(-1) ?? repository.path)}</span></TooltipTrigger>
                            <TooltipContent className="max-w-sm break-all">{visibleText(repository.path)}</TooltipContent>
                          </Tooltip></TooltipProvider>}
                          detail={`${repository.branch} · ${repository.ahead} ahead, ${repository.behind} behind`}
                          actions={<><FolderActions editor={editor} path={repository.path} onOpen={() => onOpenEditor(computerTarget(computer), repository.path)} disabled={computer.state !== "running" || computer.freshness !== "fresh"} /><ComputerBadge name={computer.configuration.name} state={computer.state} device={computer.device} /></>}
                        />
                        {(operation || repository.ahead > 0) && (
                          <div className="flex min-h-6 items-start pr-2 pb-2 pl-10" data-repository-actions>
                            {operation
                              ? <RepositoryPushFeedback disabled={!canPush} operation={operation} computer={computerTarget(computer)} repositoryPath={repository.path} repository={repository} onPush={push} onDismiss={onDismissRepositoryPush} />
                              : <RepositoryPushButton repository={repository} disabled={!canPush} label={`Push ${commitLabel(repository.ahead)} for ${repository.path} in ${label}`} onPush={push}>Push {commitLabel(repository.ahead)}</RepositoryPushButton>}
                          </div>
                        )}
                      </div>
                    )
                  })}
                </ListCard>
              ) : <EmptyState icon={<GitBranch />} title="No repositories checked out" className="min-h-24" />}
            </div>
          </CollapsibleContent>
        </section>
      </Collapsible>

      <Collapsible
        asChild
        open={fileTreeOpen}
        onOpenChange={setFileTreeOpen}
      >
        <section
          aria-label="File tree"
          className={cn(
            "collapsible-motion flex min-h-0 min-w-0 max-h-full flex-1 flex-col overflow-hidden transition-[max-height] duration-100 ease-out lg:h-full lg:max-h-none lg:border-l lg:border-border lg:pl-5 lg:transition-none",
            !fileTreeOpen && "max-h-8",
          )}
          data-files-pane="file-tree"
          data-pane-position="bottom"
        >
          <DisclosureHeader
            className="h-8 shrink-0 px-2 py-0"
            title="File tree"
            titleClassName="text-sm font-medium"
            label={`${fileTreeOpen ? "Collapse" : "Expand"} file tree`}
            controlsLabel="File tree pane controls"
          />
          <CollapsibleContent className="file-pane-content-motion min-h-0 flex-1" data-files-pane-content="file-tree">
            <div className="h-full overflow-y-auto overscroll-contain px-2 pt-2" data-files-pane-scroll="file-tree">
              <ul className="grid gap-0.5" aria-label="File tree">
                {computers.map((computer) => <ComputerFileTree editor={editor} key={computer.configuration.id} computer={computer} store={directoryStore} active={active} onOpenEditor={onOpenEditor} transfers={transfers} />)}
              </ul>
            </div>
          </CollapsibleContent>
        </section>
      </Collapsible>
    </div>
  )
}

const activityCategoryOptions: ReadonlyArray<FilterOption<ApplicationActivityCategory>> = [
  { value: "computer", label: "Computer" },
  { value: "git", label: "Git" },
  { value: "backup", label: "Export & import" },
  { value: "secrets", label: "Secrets" },
  { value: "github", label: "GitHub" },
  { value: "system", label: "System" },
]

const activityCategoryPresentation = {
  computer: { label: "Computer", icon: Box },
  git: { label: "Git", icon: GitBranch },
  backup: { label: "Export & import", icon: Archive },
  secrets: { label: "Secrets", icon: KeyRound },
  github: { label: "GitHub", icon: Cloud },
  system: { label: "System", icon: Wrench },
} as const

const activityTimeFormat = new Intl.DateTimeFormat(undefined, { dateStyle: "short", timeStyle: "medium" })
const formatActivityTime = (occurredAt: string) => activityTimeFormat.format(new Date(occurredAt))

const ActivityRow = memo(function ActivityRow({ item, computer, onShowLogs }: { item: ApplicationActivity; computer: ApplicationComputer | undefined; onShowLogs: (activity: ApplicationActivity) => void }) {
  const category = activityCategoryPresentation[item.category]
  const CategoryIcon = category.icon
  return (
    <ListRow
      role="listitem"
      aria-busy={item.status === "running" || undefined}
      data-activity-id={item.id}
      data-activity-status={item.status}
      className={cn(
        "select-text",
        item.status === "running" ? "bg-primary/5 row-hover" : item.tone === "warning" || item.tone === "danger" ? statusTones[item.tone].row : "row-hover",
      )}
      icon={
        <ListRowIcon aria-hidden="true" className={item.tone === "neutral" ? undefined : statusTones[item.tone].chip}>
          {item.status === "running"
            ? <Spinner className="text-primary" />
            : item.tone === "danger"
              ? <CircleAlert className="size-3.5 text-destructive" />
              : item.tone === "warning"
                ? <TriangleAlert className="size-3.5 text-warning" />
                : item.tone === "success"
                  ? <Check className="size-3.5 text-success" />
                  : <Activity className="size-3.5" />}
        </ListRowIcon>
      }
      title={<div className="min-w-0 break-words">{item.title}</div>}
      detailClassName="whitespace-normal"
      detail={
        <div className="min-w-0 space-y-1" data-activity-content>
          {/* A failure's detail can carry raw runtime output: keep it behind Details. */}
          {item.tone === "danger" && (item.detail || item.diagnostic)
            ? <ErrorDetails message={item.detail} diagnostic={item.diagnostic} />
            : <p className="whitespace-pre-wrap break-words">{item.detail}</p>}
          {computer && item.tone === "danger" && <Button size="xs" variant="outline" onClick={() => onShowLogs(item)}>Show logs</Button>}
          {item.status === "running" && item.progress !== undefined && (
            <div className="flex max-w-sm items-center gap-2 pt-1">
              <Progress value={item.progress * 100} aria-label={item.progressLabel ?? `${item.title} progress`} />
              <span className="w-8 shrink-0 text-right text-caption tabular-nums">{Math.round(item.progress * 100)}%</span>
            </div>
          )}
        </div>
      }
      actions={
        <div className="flex max-w-[40%] shrink-0 flex-col items-end gap-1" data-activity-meta>
          <time dateTime={item.occurredAt} className="text-caption text-muted-foreground">{formatActivityTime(item.occurredAt)}</time>
          <div className="flex flex-wrap justify-end gap-1">
            {item.computer && (computer
              ? <ComputerBadge name={computer.configuration.name} state={computer.state} device={computer.device} />
              : <StatusBadge indicator={<Box className="size-2.5" />} aria-label={`Computer: ${item.computer}`}>{item.computer}</StatusBadge>)}
            <StatusBadge indicator={<CategoryIcon className="size-2.5" />} aria-label={`Category: ${category.label}`}>{category.label}</StatusBadge>
          </div>
        </div>
      }
    />
  )
})

function ActivityLog({ computers, sourceActivities, filtered, onShowLogs }: { computers: ApplicationComputer[]; sourceActivities: ApplicationActivity[]; filtered: boolean; onShowLogs: (activity: ApplicationActivity) => void }) {
  const [selectedCategories, setSelectedCategories] = useState<Set<ApplicationActivityCategory>>(() => new Set())
  const computersByTarget = useMemo(() => new Map(computers.map((computer) => [computerTarget(computer), computer])), [computers])
  // Only a computer filter hides entries; without one, a deleted computer's events stay visible.
  const allActivities = useMemo(() => sourceActivities
    .filter(({ computer }) => !filtered || !computer || computersByTarget.has(computer))
    .sort((left, right) => right.occurredAt.localeCompare(left.occurredAt)), [sourceActivities, filtered, computersByTarget])
  const activities = useMemo(() => selectedCategories.size === 0
    ? allActivities
    : allActivities.filter(({ category }) => selectedCategories.has(category)), [allActivities, selectedCategories])

  if (computers.length === 0 && allActivities.length === 0) return <EmptyState icon={<Activity />} title="No recent activity" description="Computer and system activity will appear here." />

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      <FilterCombobox
        options={activityCategoryOptions}
        selectedValues={selectedCategories}
        onChange={setSelectedCategories}
        label="Activity category filters"
        inputLabel="Add category filter"
        placeholder="Add category…"
        listLabel="Available activity categories"
        selectedLabel="Selected activity categories"
        emptyMessage="No categories available."
        compact
        className="w-full shrink-0"
      />

      {activities.length > 0 ? (
        <ListCard divided className="max-h-full min-h-0 overflow-y-auto overscroll-contain" role="list" aria-label="Recent activity">
          {activities.map((item) => <ActivityRow key={item.id} item={item} computer={item.computer ? computersByTarget.get(item.computer) : undefined} onShowLogs={onShowLogs} />)}
        </ListCard>
      ) : (
        <EmptyState
          title={allActivities.length === 0 ? "No recent activity" : "No matching activity"}
          description={allActivities.length === 0 ? "Activity from these computers will appear here." : "Clear the category filters to show all activity."}
        />
      )}
    </div>
  )
}

export function ComputersPage({
  source,
  network, networkError, networkActions, onSectionChange,
  editor,
  onOpenEditor,
  directoryStore,
  active,
  computers,
  activities,
  selectedComputerIds,
  section,
  logQuery,
  repositoryPushOperations,
  browser,
  onComputerFilterChange,
  onLogQueryChange,
  onPushRepository,
  onDismissRepositoryPush,
  onCreateComputer,
  operationQueue,
  notifyOperations = true,
}: {
  /** The application source, for the same availability rules as the other surfaces. */
  source: ApplicationSource
  onSectionChange: (section: ComputerDetailSection) => void
  network?: ApplicationSource["network"]
  networkError?: string | null
  networkActions: ApplicationActions
  editor: string
  onOpenEditor: (computer: string, path: string) => void
  directoryStore: ReturnType<typeof createDirectoryStore>
  active: boolean
  computers: ApplicationComputer[]
  activities: ApplicationActivity[]
  selectedComputerIds: ReadonlySet<string>
  section: ComputerDetailSection
  logQuery: string
  repositoryPushOperations: RepositoryPushOperation[]
  browser: string
  onComputerFilterChange: (selectedComputerIds: Set<string>) => void
  onLogQueryChange: (query: string) => void
  onPushRepository: PushRepository
  onDismissRepositoryPush: (computer: string, repositoryPath: string) => void
  /** Opens the new-computer editor; omitted while a computer cannot be created. */
  onCreateComputer?: () => void
  /** Standalone pages own notifications; ApplicationApp owns them across navigation. */
  notifyOperations?: boolean
  /** Lets a running push be cancelled from its notification. */
  operationQueue?: OperationQueue
}) {
  const [logWindow, setLogWindow] = useState<LogWindow>()
  useRepositoryPushToasts(repositoryPushOperations, {
    enabled: notifyOperations,
    onPush: onPushRepository,
    onDismiss: onDismissRepositoryPush,
    queue: operationQueue,
    onCancel: networkActions.cancelOperation,
    resolveComputer: (target) => {
      const configuration = computers.find((computer) => computerTarget(computer) === target)?.configuration
      return configuration ? { id: configuration.id, name: configuration.name } : undefined
    },
  })
  const visibleComputers = useMemo(
    () => selectedComputerIds.size === 0 ? computers : computers.filter(({ configuration }) => selectedComputerIds.has(configuration.id)),
    [computers, selectedComputerIds],
  )
  const showActivityLogs = useStableCallback((activity: ApplicationActivity) => {
    const computer = computers.find(item => computerTarget(item) === activity.computer)
    if (computer) onComputerFilterChange(new Set([computer.configuration.id]))
    const time = new Date(activity.occurredAt).getTime()
    setLogWindow({ since: new Date(time - 5 * 60000).toISOString(), until: new Date(time + 5 * 60000).toISOString() })
    onLogQueryChange("")
    onSectionChange("logs")
  })
  // The logs reload when the set of computers changes, not when it is reordered or refreshed.
  const logTargetsKey = useMemo(() => visibleComputers.map(computerTarget).sort().join("\n"), [visibleComputers])
  // An empty filter means every computer, so an empty list means there are none yet: offer to
  // create one. Activity still shows system events and those of deleted computers.
  const hasComputers = computers.length > 0
  if (!hasComputers && section !== "activity") {
    return (
      <div className="mx-auto w-full max-w-4xl px-4 py-5 sm:px-6 sm:py-6">
        <EmptyState
          icon={<Boxes />}
          title="No computers yet"
          description="Create a computer to browse its files, logs and network ports here."
          action={onCreateComputer && <Button variant="outline" size="xs" onClick={onCreateComputer}><Plus aria-hidden="true" data-icon="inline-start" />New computer</Button>}
        />
      </div>
    )
  }

  return (
    <div className={cn("mx-auto grid h-full min-h-0 w-full max-w-4xl gap-4 overflow-hidden px-4 py-5 sm:px-6 sm:py-6", hasComputers ? "grid-rows-[auto_minmax(0,1fr)]" : "grid-rows-[minmax(0,1fr)]")}>
      {hasComputers && <ComputerFilterBar computers={computers} selectedComputerIds={selectedComputerIds} onChange={onComputerFilterChange} />}
      {section === "files" && <Files source={source} onRefreshRepositories={networkActions.refreshRepositories} editor={editor} onOpenEditor={onOpenEditor} directoryStore={directoryStore} active={active} computers={visibleComputers} repositoryPushOperations={repositoryPushOperations} onPushRepository={onPushRepository} onDismissRepositoryPush={onDismissRepositoryPush} />}
      {section === "logs" && <Suspense fallback={null}><Logs key={logTargetsKey} computers={visibleComputers} query={logQuery} onQueryChange={onLogQueryChange} actions={networkActions} active={active} window={logWindow} onWindowChange={setLogWindow} /></Suspense>}
      {section === "network" && <Suspense fallback={null}><NetworkPage computers={visibleComputers} browser={browser} network={network} error={networkError} actions={networkActions} active={active} /></Suspense>}
      {section === "activity" && <ActivityLog computers={visibleComputers} sourceActivities={activities} filtered={selectedComputerIds.size > 0} onShowLogs={showActivityLogs} />}
    </div>
  )
}

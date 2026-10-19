import { useCallback, useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react"
import { Check, ExternalLink, GitBranch, Info, RotateCcw, Search, Trash2, X } from "lucide-react"

import { ListCard, ListRow, ListRowIcon } from "@/components/list-row"
import { DisclosureHeader } from "@/components/disclosure-header"
import { ConfirmPopover } from "@/components/confirm-popover"
import { EmptyState } from "@/components/empty-state"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Collapsible, CollapsibleContent } from "@/components/ui/collapsible"
import { Input } from "@/components/ui/input"
import { Popover, PopoverAnchor, PopoverContent } from "@/components/ui/popover"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip"
import { Spinner } from "@/components/ui/spinner"

export type GitHubConnectionState = "disconnected" | "connecting" | "connected"

export interface GitHubRepositorySelection {
  repository: string
  allowPushes: boolean
}

const emptySelections: readonly GitHubRepositorySelection[] = []

export interface GitHubRepositoryAccess {
  authenticationMethod?: "oauth" | "token"
  repositoryMode: "selected" | "all"
  allRepositoriesAllowChanges: boolean
}

export interface GitHubIdentity {
  name: string
  email: string
  apply: boolean
}

export interface GitHubComputer {
  name: string
}

interface RepositoryComboboxProps {
  computer: string
  repositoryOptions: readonly string[]
  selectedRepositories: readonly GitHubRepositorySelection[]
  disabled?: boolean
  onManageRepositories?: () => void
  onAdd: (repository: string) => void
}

function ComputerDisclosure({ name, actions, children }: { name: string; actions?: ReactNode; children: ReactNode }) {
  const [open, setOpen] = useState(true)

  return (
    <Collapsible open={open} onOpenChange={setOpen} className="collapsible-motion">
      <DisclosureHeader
        className="h-10 px-3 py-0"
        title={<span title={name}>{name}</span>}
        titleClassName="text-xs font-semibold"
        label={`${open ? "Collapse" : "Expand"} ${name}`}
        actions={actions && <div className="flex shrink-0 items-center gap-1.5">{actions}</div>}
      />
      <CollapsibleContent className="collapsible-content-motion">
        {children}
      </CollapsibleContent>
    </Collapsible>
  )
}

const repositoryGridColumns = "grid-cols-[minmax(0,1fr)_10rem_1.5rem]"

function RepositoryCombobox({ computer, repositoryOptions, selectedRepositories, disabled = false, onAdd, onManageRepositories }: RepositoryComboboxProps) {
  const listboxId = useId()
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState("")
  const [activeOption, setActiveOption] = useState<string>()
  const activeElement = useRef<HTMLButtonElement>(null)
  const selectedNames = useMemo(
    () => new Set(selectedRepositories.map(({ repository }) => repository.toLowerCase())),
    [selectedRepositories],
  )
  const matchingRepositories = useCallback((searchQuery: string) => {
    const normalizedQuery = searchQuery.trim().toLowerCase()
    return repositoryOptions.filter((repository) => {
      const name = repository.toLowerCase()
      return !selectedNames.has(name) && name.includes(normalizedQuery)
    })
  }, [repositoryOptions, selectedNames])
  const results = useMemo(() => matchingRepositories(query), [matchingRepositories, query])

  const searchActions = [
    ...(onManageRepositories ? [{ label: "Add more repositories on GitHub", run: onManageRepositories, icon: ExternalLink }] : []),
  ]
  const optionNames = [...results, ...searchActions.map(({ label }) => label)]
  const activeIndex = activeOption === undefined ? -1 : optionNames.indexOf(activeOption)
  useEffect(() => {
    if (open) activeElement.current?.scrollIntoView({ block: "nearest", inline: "nearest" })
  }, [open, activeIndex, activeOption])

  function openResults() {
    if (!open) setActiveOption(optionNames[0])
    setOpen(true)
  }

  function runAction(index: number) {
    searchActions[index]?.run()
    setOpen(false)
    setActiveOption(undefined)
  }

  function add(repository: string) {
    if (disabled || selectedNames.has(repository.toLowerCase())) return
    onAdd(repository)
    setQuery("")
    setActiveOption(undefined)
    setOpen(false)
  }

  return (
    <Popover open={!disabled && open} onOpenChange={(nextOpen) => !disabled && setOpen(nextOpen)}>
      <PopoverAnchor asChild>
        <div className="relative">
          <Search aria-hidden="true" className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input technical
            role="combobox"
            aria-label={`Add repository to ${computer}`}
            aria-autocomplete="list"
            aria-expanded={!disabled && open}
            aria-controls={listboxId}
            aria-activedescendant={!disabled && open && activeIndex >= 0 ? `${listboxId}-${activeIndex}` : undefined}
            className="pl-8 text-xs"
            disabled={disabled}
            placeholder="Search repositories…"
            value={query}
            onFocus={openResults}
            onClick={openResults}
            onChange={(event) => {
              setQuery(event.target.value)
              setActiveOption(matchingRepositories(event.target.value)[0] ?? searchActions[0]?.label)
              setOpen(true)
            }}
            onKeyDown={(event) => {
              if (event.nativeEvent.isComposing) return
              if (event.key === "ArrowDown") {
                event.preventDefault()
                setOpen(true)
                setActiveOption(optionNames[Math.min(activeIndex + 1, Math.max(0, optionNames.length - 1))])
              } else if (event.key === "ArrowUp") {
                event.preventDefault()
                setActiveOption(optionNames[Math.max(0, activeIndex - 1)])
              } else if (event.key === "Enter" && open && activeIndex >= 0) {
                event.preventDefault()
                if (results[activeIndex]) add(results[activeIndex])
                else runAction(activeIndex - results.length)
              } else if (event.key === "Escape") {
                setOpen(false)
              }
            }}
          />
        </div>
      </PopoverAnchor>
      <PopoverContent
        id={listboxId}
        role="listbox"
        aria-label={`Repository results for ${computer}`}
        className="max-h-[min(15rem,var(--radix-popover-content-available-height))] w-[var(--radix-popover-trigger-width)] overflow-y-auto overscroll-contain p-1"
        onOpenAutoFocus={(event) => event.preventDefault()}
        onEscapeKeyDown={(event) => { if (event.isComposing) event.preventDefault() }}
      >
        {results.length > 0 ? results.map((repository, index) => (
          <button
            ref={index === activeIndex ? activeElement : undefined}
            key={repository}
            id={`${listboxId}-${index}`}
            type="button"
            role="option"
            tabIndex={-1}
            aria-selected={index === activeIndex}
            className="flex w-full items-center rounded-sm px-2 py-1.5 text-left text-xs outline-none hover:bg-accent focus:bg-accent aria-selected:bg-accent"
            disabled={disabled}
            onMouseDown={(event) => event.preventDefault()}
            onMouseEnter={() => setActiveOption(repository)}
            onClick={() => add(repository)}
          >
            <span className="min-w-0 break-all">{repository}</span>
          </button>
        )) : (
          <EmptyState variant="inline" title="No repositories found" />
        )}
        {searchActions.map((action, index) => (
          <button
            ref={activeIndex === results.length + index ? activeElement : undefined}
            key={action.label}
            id={`${listboxId}-${results.length + index}`}
            type="button"
            role="option"
            tabIndex={-1}
            aria-selected={activeIndex === results.length + index}
            className={`flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs outline-none hover:bg-accent focus:bg-accent aria-selected:bg-accent ${index === 0 ? "mt-1 border-t border-border" : ""}`}
            onMouseDown={(event) => event.preventDefault()}
            onMouseEnter={() => setActiveOption(action.label)}
            onClick={() => runAction(index)}
          >
            <action.icon aria-hidden="true" className="size-3 shrink-0" />
            {action.label}
          </button>
        ))}
      </PopoverContent>
    </Popover>
  )
}

export interface GitHubAccessEditorProps {
  computers: readonly GitHubComputer[]
  connectionState: GitHubConnectionState
  tokenConnected?: boolean
  tokenConnection?: ReactNode
  repositoryOptions: readonly string[]
  computerSelections: Readonly<Record<string, readonly GitHubRepositorySelection[]>>
  computerRepositoryAccess?: Readonly<Record<string, GitHubRepositoryAccess>>
  onComputerRepositoryAccessChange?: (computer: string, access: GitHubRepositoryAccess) => void
  computerIdentities: Readonly<Record<string, GitHubIdentity>>
  currentDeviceGitIdentity: { name: string; email: string } | null
  onCancelConnection?: () => void
  onManageRepositories?: () => void
  onReopenAuthorization?: () => void
  onConnect: () => void
  onComputerSelectionsChange: (computer: string, selections: GitHubRepositorySelection[]) => void
  onComputerIdentityChange: (computer: string, identity: GitHubIdentity) => void
  onCommitComputerIdentity?: (computer: string, identity: GitHubIdentity) => void
  onResetComputerIdentity: (computer: string) => void
  compactConnection?: boolean
  connectedTitle?: ReactNode
  connectedDetail?: ReactNode
  connectionProgress?: ReactNode
  connectedActions?: ReactNode
  notice?: ReactNode
  renderComputerActions?: (computer: GitHubComputer) => ReactNode
  renderComputerNotice?: (computer: GitHubComputer) => ReactNode
  footer?: ReactNode
  disabled?: boolean
  repositoryControlsAvailable?: boolean
  confirmRepositoryClear?: boolean
  busy?: boolean
}

export function GitHubAccessEditor({
  computers,
  connectionState,
  tokenConnected = false,
  tokenConnection,
  repositoryOptions,
  computerSelections,
  computerIdentities,
  computerRepositoryAccess = {},
  onComputerRepositoryAccessChange,
  currentDeviceGitIdentity,
  onConnect,
  onCancelConnection,
  onReopenAuthorization,
  onManageRepositories,
  onComputerSelectionsChange,
  onComputerIdentityChange,
  onCommitComputerIdentity,
  onResetComputerIdentity,
  compactConnection = false,
  connectedTitle = "Connected to GitHub",
  connectedDetail = "Repository credentials are scoped to each computer.",
  connectionProgress,
  connectedActions,
  notice,
  renderComputerActions,
  renderComputerNotice,
  footer,
  disabled = false,
  repositoryControlsAvailable = true,
  confirmRepositoryClear = false,
  busy = false,
}: GitHubAccessEditorProps) {

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3">
      <ListCard
        className={compactConnection || connectionState !== "connected" ? "shrink-0" : "shrink-0 overflow-visible rounded-none border-0"}
        role={connectionState === "connecting" ? "status" : undefined}
        aria-live={connectionState === "connecting" ? "polite" : undefined}
      >
        <ListRow
          className={`grid grid-cols-[auto_minmax(0,1fr)] gap-y-2 sm:flex ${compactConnection ? "row-hover" : connectionState === "connected" ? "gap-x-3 p-0" : "gap-x-3 p-4"}`}
          icon={
            <ListRowIcon
              aria-hidden="true"
              className={`${compactConnection ? "" : "size-9 rounded-full"} ${connectionState === "connected" ? "bg-success/10 text-success" : ""}`}
            >
              {connectionState === "connected" ? <Check className={compactConnection ? "size-3.5" : "size-4"} />
                : connectionState === "connecting" ? <Spinner className={compactConnection ? undefined : "size-4"} />
                  : <GitBranch className={compactConnection ? "size-3.5" : "size-4"} />}
            </ListRowIcon>
          }
          title={<h3 className={compactConnection ? undefined : "text-sm"}>{connectionState === "connected" ? connectedTitle : connectionState === "connecting" ? "Connecting to GitHub…" : "Not connected"}</h3>}
          detail={connectionState === "connected" ? connectedDetail : connectionState === "connecting" ? "Finish signing in in your browser." : "Connect to choose repositories and allow GitHub changes."}
          detailClassName={compactConnection ? "whitespace-normal" : "mt-0.5 whitespace-normal text-xs"}
          actions={(
            <div className={`col-start-2 flex shrink-0 flex-wrap items-center ${compactConnection ? "gap-1" : "gap-2"}`}>
              {connectionState === "connecting" ? <>
                <Button type="button" size={compactConnection ? "xs" : "default"} variant="outline" onClick={onReopenAuthorization}>Open browser again</Button>
                <Button type="button" size={compactConnection ? "xs" : "default"} variant="ghost" onClick={onCancelConnection}>Cancel</Button>
              </> : connectionState === "connected" ? connectedActions : <Button type="button" size={compactConnection ? "xs" : "default"} variant={compactConnection ? "outline" : "default"} onClick={onConnect}>Connect GitHub</Button>}
            </div>
          )}
        />
        {connectionProgress}
      </ListCard>

      {tokenConnection}
      {notice && <div className="shrink-0">{notice}</div>}

      <ScrollArea className="min-h-0 flex-1 rounded-md border border-border" role="region" aria-label="Computer Git identity and repository access" aria-busy={busy || undefined}>
        <div className="divide-y divide-border">
          {computers.map((computer) => {
            const { name } = computer
            const selections = (Object.hasOwn(computerSelections, name) ? computerSelections[name] : undefined) ?? emptySelections
            const access = (Object.hasOwn(computerRepositoryAccess, name) ? computerRepositoryAccess[name] : undefined) ?? { repositoryMode: "selected", allRepositoriesAllowChanges: false }
            const identity = (Object.hasOwn(computerIdentities, name) ? computerIdentities[name] : undefined) ?? { name: "", email: "", apply: true }
            const computerActions = renderComputerActions?.(computer)
            const computerNotice = renderComputerNotice?.(computer)
            const computerDisabled = disabled
            return (
              <ComputerDisclosure key={name} name={name} actions={computerActions}>
                  <div className="grid gap-3 px-3 pb-3">
                    {computerNotice}
                    <div
                      role="group"
                      aria-label={`Git identity for ${name}`}
                      data-layout="compact-row"
                      className="flex min-w-0 items-center gap-1.5"
                    >
                      <TooltipProvider delayDuration={150}>
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <span
                              tabIndex={0}
                              aria-label={`About Git identity for ${name}`}
                              className="grid size-4 shrink-0 place-items-center rounded-sm text-muted-foreground focus-ring"
                            >
                              <GitBranch aria-hidden="true" className="size-3.5" />
                            </span>
                          </TooltipTrigger>
                          <TooltipContent>Name and email used for Git commits in this computer.</TooltipContent>
                        </Tooltip>
                      </TooltipProvider>
                      <Input size="sm" technical
                        aria-label={`Git name for ${name}`}
                        autoComplete="off"
                        className="flex-[0.8]"
                        placeholder="Name"
                        disabled={computerDisabled}
                        value={identity.name}
                        onChange={(event) => onComputerIdentityChange(name, { ...identity, name: event.target.value })}
                        onBlur={() => onCommitComputerIdentity?.(name, identity)}
                        onKeyDown={(event) => {
                          if (event.key === "Enter" && !event.nativeEvent.isComposing) event.currentTarget.blur()
                        }}
                      />
                      <Input size="sm" technical
                        aria-label={`Git email for ${name}`}
                        autoComplete="off"
                        className="flex-[1.2]"
                        inputMode="email"
                        placeholder="Email"
                        type="email"
                        disabled={computerDisabled}
                        value={identity.email}
                        onChange={(event) => onComputerIdentityChange(name, { ...identity, email: event.target.value })}
                        onBlur={() => onCommitComputerIdentity?.(name, identity)}
                        onKeyDown={(event) => {
                          if (event.key === "Enter" && !event.nativeEvent.isComposing) event.currentTarget.blur()
                        }}
                      />
                      <label className="flex shrink-0 items-center gap-1 text-caption">
                        <Checkbox
                          aria-label={`Apply Git identity to ${name}`}
                          checked={identity.apply}
                          disabled={computerDisabled}
                          onCheckedChange={(checked) => onComputerIdentityChange(name, { ...identity, apply: checked === true })}
                        />
                        Apply
                      </label>
                      <TooltipProvider delayDuration={150}>
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <span
                              className="inline-flex shrink-0"
                              tabIndex={currentDeviceGitIdentity ? undefined : 0}
                              aria-label={currentDeviceGitIdentity ? undefined : `Reset Git identity for ${name}`}
                            >
                              <Button
                                type="button"
                                variant="ghost"
                                size="icon-xs"
                                aria-label={`Reset Git identity for ${name}`}
                                disabled={computerDisabled || !currentDeviceGitIdentity}
                                onClick={() => onResetComputerIdentity(name)}
                              >
                                <RotateCcw aria-hidden="true" className="size-3" />
                              </Button>
                            </span>
                          </TooltipTrigger>
                          <TooltipContent>{`Reset Git identity for ${name}`}</TooltipContent>
                        </Tooltip>
                      </TooltipProvider>
                    </div>
                    {onComputerRepositoryAccessChange && (
                      <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
                        <div role="radiogroup" aria-label={`GitHub authentication for ${name}`} className="flex flex-wrap items-center gap-4 text-xs">
                          <label className="flex items-center gap-2">
                            <input type="radio" name={`github-method-${name}`} aria-label={`Use GitHub OAuth for ${name}`}
                              checked={(access.authenticationMethod ?? "oauth") === "oauth"}
                              disabled={computerDisabled || connectionState !== "connected"}
                              onChange={() => onComputerRepositoryAccessChange(name, { ...access, authenticationMethod: "oauth" })} />
                            Use GitHub OAuth
                          </label>
                          <TooltipProvider><Tooltip><TooltipTrigger asChild>
                            <label className="flex items-center gap-2">
                              <input type="radio" name={`github-method-${name}`} aria-label={`Use token for ${name}`}
                                checked={access.authenticationMethod === "token"}
                                disabled={computerDisabled || !tokenConnected}
                                onChange={() => onComputerRepositoryAccessChange(name, { ...access, authenticationMethod: "token" })} />
                              Use token
                            </label>
                          </TooltipTrigger><TooltipContent>Full token access. This computer can perform every action permitted by the token, with no additional Silo repository restrictions. Credentials remain outside the computer.</TooltipContent></Tooltip></TooltipProvider>
                        </div>
                        {connectionState === "connected" && access.authenticationMethod !== "token" && (
                          <div className="ml-auto flex flex-wrap items-center justify-end gap-x-4 gap-y-2 text-xs">
                            <label className="flex items-center gap-2">
                              <Checkbox aria-label={`All repositories for ${name}`} checked={access.repositoryMode === "all"} disabled={computerDisabled || !repositoryControlsAvailable}
                                onCheckedChange={(checked) => onComputerRepositoryAccessChange(name, { ...access, repositoryMode: checked === true ? "all" : "selected", allRepositoriesAllowChanges: false })} />
                              All repositories
                            </label>
                            {access.repositoryMode === "all" && <label className="flex items-center gap-2">
                              <Checkbox aria-label={`Allow GitHub changes for all repositories in ${name}`} checked={access.allRepositoriesAllowChanges} disabled={computerDisabled || !repositoryControlsAvailable}
                                onCheckedChange={(checked) => onComputerRepositoryAccessChange(name, { ...access, allRepositoriesAllowChanges: checked === true })} />
                              Allow GitHub changes
                            </label>}
                          </div>
                        )}
                      </div>
                    )}
                    {connectionState === "connected" && access.authenticationMethod !== "token" && (
                      <>
                        {access.repositoryMode === "all" && <p className="text-xs text-muted-foreground">All repositories authorized on GitHub, including future additions.</p>}
                        {access.repositoryMode !== "all" && repositoryControlsAvailable && (
                          <RepositoryCombobox
                            computer={name}
                            repositoryOptions={repositoryOptions}
                            selectedRepositories={selections}
                            onManageRepositories={onManageRepositories}
                            disabled={computerDisabled}
                            onAdd={(repository) => onComputerSelectionsChange(name, [...selections, { repository, allowPushes: false }])}
                          />
                        )}
                        {access.repositoryMode !== "all" && selections.length > 0 && (
                          <div role="table" aria-label={`Selected repositories for ${name}`} className="overflow-hidden rounded-md border border-border">
                            <div role="row" className={`grid ${repositoryGridColumns} items-center gap-2 bg-muted/50 px-2 py-1.5 text-left text-caption font-medium text-muted-foreground`}>
                              <span role="columnheader">Repository</span>
                              <span role="columnheader" className="flex items-center justify-start gap-0.5 text-left">
                                Allow GitHub changes
                                <TooltipProvider delayDuration={150}>
                                  <Tooltip>
                                    <TooltipTrigger asChild>
                                      <Button type="button" variant="ghost" size="icon-xs" className="size-5" aria-label="About Allow GitHub changes">
                                        <Info aria-hidden="true" className="size-3" />
                                      </Button>
                                    </TooltipTrigger>
                                    <TooltipContent>Allow Git pushes and GitHub changes, such as issues and pull requests, from this computer.</TooltipContent>
                                  </Tooltip>
                                </TooltipProvider>
                              </span>
                              <span role="columnheader" className="flex justify-start">
                                <TooltipProvider delayDuration={150}>
                                  {(() => {
                                    const trigger = <Button
                                      type="button"
                                      variant="ghost"
                                      size="icon-xs"
                                      className="size-5"
                                      aria-label={`Clear repositories from ${name}`}
                                      disabled={computerDisabled || !repositoryControlsAvailable}
                                      onClick={confirmRepositoryClear ? undefined : () => onComputerSelectionsChange(name, [])}
                                    >
                                      <Trash2 aria-hidden="true" className="size-3" />
                                    </Button>
                                    const label = `Clear repositories from ${name}`
                                    return confirmRepositoryClear
                                      ? <ConfirmPopover align="end" tone="destructive" title={`Remove all repositories from ${name}?`} description={`${name} loses GitHub access to them.`} confirmLabel="Remove all" tooltip={label} onConfirm={() => onComputerSelectionsChange(name, [])}>{trigger}</ConfirmPopover>
                                      : <Tooltip><TooltipTrigger asChild><span className="inline-flex">{trigger}</span></TooltipTrigger><TooltipContent>{label}</TooltipContent></Tooltip>
                                  })()}
                                </TooltipProvider>
                              </span>
                            </div>
                            {selections.map((selection) => (
                              <div key={selection.repository} role="row" className={`grid ${repositoryGridColumns} items-center gap-2 border-t border-border px-2 py-2 text-left`}>
                                <span role="cell" className="min-w-0 break-all text-xs">{selection.repository}</span>
                                <span role="cell" className="flex justify-start">
                                  <Checkbox
                                    aria-label={`Allow GitHub changes for ${selection.repository}`}
                                    checked={selection.allowPushes}
                                    disabled={computerDisabled || !repositoryControlsAvailable}
                                    onCheckedChange={(checked) => onComputerSelectionsChange(name, selections.map((item) => (
                                      item.repository === selection.repository ? { ...item, allowPushes: checked === true } : item
                                    )))}
                                  />
                                </span>
                                <span role="cell" className="flex justify-start">
                                  <Button
                                    type="button"
                                    variant="ghost"
                                    size="icon-xs"
                                    aria-label={`Remove ${selection.repository} from ${name}`}
                                    disabled={computerDisabled || !repositoryControlsAvailable}
                                    onClick={() => onComputerSelectionsChange(name, selections.filter(({ repository }) => repository !== selection.repository))}
                                  >
                                    <X aria-hidden="true" />
                                  </Button>
                                </span>
                              </div>
                            ))}
                          </div>
                        )}
                      </>
                    )}
                  </div>
              </ComputerDisclosure>
            )
          })}
        </div>
      </ScrollArea>

      {footer && <div className="shrink-0">{footer}</div>}
    </div>
  )
}

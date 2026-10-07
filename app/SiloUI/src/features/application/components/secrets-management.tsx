import { Box, Globe, KeyRound, Pencil, RotateCw, Trash2 } from "lucide-react"

import { ListRow, ListRowIcon } from "@/components/list-row"
import { ConfirmPopover } from "@/components/confirm-popover"
import { StatusBadge } from "@/components/status-badge"
import { Button } from "@/components/ui/button"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { Spinner } from "@/components/ui/spinner"
import { ComputerBadge } from "@/features/application/components/application-ui"
import { SecretEditor } from "@/features/application/components/secret-editor"
import type { ApplicationSecret } from "@/features/application/model/application-source"
import type { SecretsManager } from "@/features/application/components/secrets-manager"

/** The add-secret editor, rendered when the manager is adding a new secret. */
export function AddSecretEditor({ manager }: { manager: SecretsManager }) {
  if (!manager.editor || manager.editor.secret) return null
  return <SecretEditor
    key="add"
    source={manager.source}
    initialComputers={manager.editor.initialComputers}
    onSave={manager.saveSecret}
    onCancel={manager.closeEditor}
    saving={manager.saving}
    saveError={manager.saveError}
  />
}

/** A single secret row with its live state (applying/restart-required/removing), Edit and
 * Remove controls (Remove asks in a popover), failure/Retry, and the inline editor. */
export function SecretRow({ secret, manager }: { secret: ApplicationSecret; manager: SecretsManager }) {
  const { source } = manager
  const working = manager.busy === secret.id
  const failure = manager.operationError?.id === secret.id ? manager.operationError.message : secret.error
  const disabled = manager.saving || manager.busy !== null

  return (
    <li>
      <ListRow
        className="row-hover"
        icon={<ListRowIcon aria-hidden="true"><KeyRound className="size-3.5" /></ListRowIcon>}
        title={<div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
          <h3 className="break-all font-mono">{secret.name}</h3>
          {secret.state === "applying" && <span role="status" className="text-[10px] text-muted-foreground">{secret.removing ? "Removing…" : "Applying…"}</span>}
          {secret.state === "restart-required" && (
            <span className="inline-flex items-center gap-1 text-[10px] text-warning">
              <RotateCw className="size-3" aria-hidden="true" />Restart to apply{secret.pendingComputers?.length ? `: ${secret.pendingComputers.join(", ")}` : ""}
            </span>
          )}
          {secret.removing && secret.state !== "applying" && <span className="text-[10px] text-muted-foreground">Removal pending</span>}
        </div>}
        detailClassName="whitespace-normal"
        detail={<div className="mt-1 flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1.5">
          <div className="flex min-w-0 flex-wrap gap-1" role="group" aria-label={`Computers for ${secret.name}`}>
            {secret.computers.map((name) => {
              const computer = source.computers.find(({ configuration, device }) => !device && configuration.name === name)
              return computer
                ? <ComputerBadge key={name} name={name} state={computer.state} device={computer.device} />
                : <StatusBadge key={name} indicator={<Box className="size-2" />}>{name}</StatusBadge>
            })}
          </div>
          <p className="flex min-w-0 items-start gap-1 text-[11px] text-muted-foreground" aria-label={`Allowed domains for ${secret.name}`}>
            <Globe className="mt-0.5 size-3 shrink-0" aria-hidden="true" />
            <span className="break-all">{secret.allowedDomains.join(", ") || "No allowed domains"}</span>
          </p>
        </div>}
        actions={<div className="flex shrink-0 items-center gap-0.5 text-muted-foreground" role="group" aria-label={`Manage ${secret.name}`}>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button type="button" variant="ghost" size="icon-xs" aria-label={`Edit ${secret.name}`} disabled={disabled} onClick={(event) => manager.openEditor(event.currentTarget, { secret })}>
                <Pencil aria-hidden="true" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>{`Edit ${secret.name}`}</TooltipContent>
          </Tooltip>
          <ConfirmPopover align="end" tone="destructive" title={`Remove ${secret.name}?`} description="Silo deletes the stored value immediately. Computers that cannot revoke access may keep it until they restart." confirmLabel="Remove" tooltip={`Remove ${secret.name}`} onConfirm={() => manager.removeSecret(secret.id)}>
            <Button type="button" variant="ghost" size="icon-xs" aria-label={`Remove ${secret.name}`} disabled={disabled}>
              {working ? <Spinner /> : <Trash2 aria-hidden="true" />}
            </Button>
          </ConfirmPopover>
        </div>}
      />
      {failure && <div className="flex items-center justify-between gap-3 px-3 pb-3 text-[11px] text-destructive">
        <p role="alert">{failure}</p>
        <Button variant="outline" size="xs" disabled={disabled || (!manager.onRetrySecret && manager.operationError?.id !== secret.id)} onClick={() => { const action = manager.operationError?.id === secret.id ? manager.operationError.action : manager.onRetrySecret; if (action) void manager.runOperation(secret.id, action) }}>
          {working && <Spinner />}Retry
        </Button>
      </div>}
      {manager.editor?.secret?.id === secret.id && <div className="border-t border-border"><SecretEditor key={secret.id} secret={secret} source={source} onSave={manager.saveSecret} onCancel={manager.closeEditor} saving={manager.saving} saveError={manager.saveError} /></div>}
    </li>
  )
}

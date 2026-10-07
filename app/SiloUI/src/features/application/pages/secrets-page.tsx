import { KeyRound, Plus } from "lucide-react"

import { EmptyState } from "@/components/empty-state"
import { ListCard } from "@/components/list-row"
import { Button } from "@/components/ui/button"
import { TooltipProvider } from "@/components/ui/tooltip"
import { AddSecretEditor, SecretRow } from "@/features/application/components/secrets-management"
import { useSecretsManager } from "@/features/application/components/secrets-manager"
import type { ApplicationSource, SecretConfigurationRequest } from "@/features/application/model/application-source"

export function SecretsPage({ source, onSaveSecret, onRemoveSecret, onRetrySecret }: {
  source: ApplicationSource
  onSaveSecret: (request: SecretConfigurationRequest) => Promise<void> | void
  onRemoveSecret: (id: string) => Promise<void> | void
  onRetrySecret?: (id: string) => Promise<void> | void
}) {
  const secrets = source.secrets
  const manager = useSecretsManager({ source, onSaveSecret, onRemoveSecret, onRetrySecret })

  return (
    <div className="mx-auto grid w-full max-w-4xl gap-2 px-4 py-5 sm:px-6 sm:py-6">
      <header className="flex min-w-0 flex-wrap items-center justify-between gap-2">
        <div className="min-w-0">
          <h2 className="text-xs font-medium">Secrets</h2>
          <p className="text-caption text-muted-foreground"><span>{secrets.length} configured</span> · This device</p>
        </div>
        <Button type="button" variant="outline" size="xs" aria-label="Add secret" disabled={manager.saving || manager.busy !== null} onClick={(event) => manager.openEditor(event.currentTarget)}>
          <Plus aria-hidden="true" data-icon="inline-start" /> Add
        </Button>
      </header>
      {manager.editor && !manager.editor.secret && <ListCard><AddSecretEditor manager={manager} /></ListCard>}
      {secrets.length > 0 ? (
        <TooltipProvider delayDuration={150}>
          <ListCard>
            <ul className="divide-y divide-border" aria-label="Configured secrets">
              {secrets.map((secret) => <SecretRow key={secret.id} secret={secret} manager={manager} />)}
            </ul>
          </ListCard>
        </TooltipProvider>
      ) : <EmptyState icon={<KeyRound />} title="No secrets configured." />}
    </div>
  )
}

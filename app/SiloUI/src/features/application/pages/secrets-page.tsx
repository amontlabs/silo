import { KeyRound, Plus } from "lucide-react"

import { EmptyState } from "@/components/empty-state"
import { ListCard } from "@/components/list-row"
import { Button } from "@/components/ui/button"
import { TooltipProvider } from "@/components/ui/tooltip"
import { PageContainer, PageHeader } from "@/components/page"
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
    <PageContainer className="grid gap-2">
      <PageHeader
        title="Secrets"
        subtitle={<><span>{secrets.length} configured</span> · This device</>}
        actions={<Button type="button" variant="outline" size="xs" aria-label="Add secret" disabled={manager.saving || manager.busy !== null} onClick={(event) => manager.openEditor(event.currentTarget)}>
          <Plus aria-hidden="true" data-icon="inline-start" /> Add
        </Button>}
      />
      {manager.editor && !manager.editor.secret && <ListCard><AddSecretEditor manager={manager} /></ListCard>}
      {secrets.length > 0 ? (
        <TooltipProvider delayDuration={150}>
          <ListCard>
            <ul className="divide-y divide-border" aria-label="Configured secrets">
              {secrets.map((secret) => <SecretRow key={secret.id} secret={secret} manager={manager} />)}
            </ul>
          </ListCard>
        </TooltipProvider>
      ) : <EmptyState icon={<KeyRound />} title="No secrets configured" />}
    </PageContainer>
  )
}

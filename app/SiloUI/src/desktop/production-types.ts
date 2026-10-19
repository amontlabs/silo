import type { z } from "zod"

import type { SetupComputerConfiguration, SiloProgressEvent, SetupComputerConfigurationRequest } from "@/contracts/silo"
import type { OnboardingSource } from "@/features/onboarding/model/onboarding-source"
import type { ApplicationSource } from "@/features/application/model/application-source"
import type { BackupState } from "@/features/application/model/backup-source"
import type { deviceSchema } from "@/features/application/model/connections"

export type EventHandler = (event?: { payload: unknown }) => void

export interface ProductionBridge {
  invoke: <T>(command: string, arguments_?: Record<string, unknown>) => Promise<T>
  listen: (event: string, handler: EventHandler) => Promise<() => void>
}

export interface ProductionSnapshot {
  savedConfigurations?: SetupComputerConfiguration[]
  setupQueue: NonNullable<OnboardingSource["setupQueue"]>
  setupStartedAt?: number
  setupFinishedAt?: number
  setupEvents: SiloProgressEvent[]
  setupActivity?: SiloProgressEvent[]
  setupActivityError?: string
  setupCandidate?: SetupComputerConfigurationRequest
  /** The setup work Quit is waiting for while it drains setup. */
  setupDrain?: string
  /** `source` is a shell for connected devices while this device's computers update. */
  localUpdating?: boolean
  source: ApplicationSource | null
  backup: BackupState
  loading: boolean
  error: string | null
}

export type ListedDevice = z.infer<typeof deviceSchema>
export type LifecycleAction = "start" | "stop" | "restart"

import type { ApplicationSecret, SecretConfigurationRequest, SecretKind } from "./application-source"
import { hostWarning } from "@/features/network-policy/model/network-policy"
import reservedSecretNames from "./reserved-secret-names.json"

export interface SecretDraft {
  kind: SecretKind
  name: string
  value: string
  computers: string[]
  domains: string
  allowAnyDomain: boolean
  allowSharedHosting: boolean
}

export type SecretValidationErrors = Partial<Record<keyof SecretDraft, string>>

// The native secrets controller reads the same list, so both reject the same names.
const reservedNames = new Set<string>(reservedSecretNames.names)

function reservedName(name: string) {
  const upper = name.toUpperCase()
  return reservedNames.has(upper) || reservedSecretNames.prefixes.some((prefix) => upper.startsWith(prefix))
}

function validDomain(domain: string) {
  if (domain === "*") return true
  const wildcard = domain.startsWith("*.")
  const host = wildcard ? domain.slice(2) : domain
  const labels = host.split(".")
  return host.length <= 253 && (!wildcard || labels.length >= 2) && labels.every((label) =>
    /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/.test(label))
}

/** The hosts among `domains` that lead to sites anyone can publish, each with what that allows. */
export function sharedHostingWarnings(domains: readonly string[]): string[] {
  return domains.flatMap((domain) => hostWarning(domain) ?? [])
}

export function secretConfiguration(draft: SecretDraft, secrets: readonly ApplicationSecret[], availableComputers: readonly string[], original?: ApplicationSecret):
  { request: SecretConfigurationRequest; errors?: never } | { errors: SecretValidationErrors; request?: never } {
  const errors: SecretValidationErrors = {}
  const name = original?.name ?? draft.name.trim()
  if ((original?.kind ?? draft.kind) === "github-token") return githubTokenConfiguration(draft, secrets, name, original)
  const allowedDomains = [...new Set(draft.domains.split(/[\s,]+/).filter(Boolean).map((domain) => domain.toLowerCase()))]

  if (!/^[A-Za-z_][A-Za-z0-9_]{0,127}$/.test(name)) {
    errors.name = "Use up to 128 letters, digits, or underscores, starting with a letter or underscore."
  } else if (reservedName(name)) {
    errors.name = "This name is reserved. Choose another name."
  } else if (secrets.some((secret) => secret.id !== original?.id && secret.name === name)) {
    errors.name = "A secret with this name already exists."
  }
  if (!original && !draft.value) errors.value = "Enter a value."
  else if (draft.value.includes("\0")) errors.value = "Secret values cannot contain null characters."
  else if (new TextEncoder().encode(draft.value).byteLength > 65536) errors.value = "Use a secret value of at most 64 KiB."
  if (draft.computers.length === 0) errors.computers = "Select at least one computer."
  else if (draft.computers.length > 100) errors.computers = "Select no more than 100 computers."
  else if (draft.computers.some((name) => !availableComputers.includes(name))) errors.computers = "Select an available computer."
  if (allowedDomains.length === 0) errors.domains = "Enter at least one allowed domain."
  else if (allowedDomains.length > 100) errors.domains = "Use no more than 100 allowed domains."
  else if (!allowedDomains.every(validDomain)) errors.domains = "Use hosts such as api.example.com or *.example.com, without a scheme, port, or path."
  if (allowedDomains.includes("*") && !draft.allowAnyDomain) errors.allowAnyDomain = "Confirm access to any HTTPS destination."
  if (Object.keys(errors).length > 0) return { errors }

  const confirmed = draft.allowSharedHosting && sharedHostingWarnings(allowedDomains).length > 0
  const metadata = { name, computers: [...draft.computers], allowedDomains, ...(confirmed ? { allowSharedHosting: true as const } : {}) }
  return { request: original
    ? { ...metadata, operation: "edit", id: original.id, ...(draft.value ? { value: draft.value } : {}) }
    : { ...metadata, operation: "add", value: draft.value } }
}

/** A GitHub token takes a label and a value. Its computers are chosen in GitHub access and
 * its hosts are GitHub's, so neither is asked for here. */
function githubTokenConfiguration(draft: SecretDraft, secrets: readonly ApplicationSecret[], name: string, original?: ApplicationSecret):
  { request: SecretConfigurationRequest; errors?: never } | { errors: SecretValidationErrors; request?: never } {
  const errors: SecretValidationErrors = {}
  const value = draft.value.trim()
  if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(name)) {
    errors.name = "Use up to 64 letters, digits, hyphens or underscores, starting with a letter or digit."
  } else if (secrets.some((secret) => secret.id !== original?.id && secret.name === name)) {
    errors.name = "A secret with this name already exists."
  }
  if (!original && !value) errors.value = "Enter a token."
  else if (value && !/^[A-Za-z0-9_]{1,4096}$/.test(value)) errors.value = "Enter a GitHub personal access token."
  if (Object.keys(errors).length > 0) return { errors }

  const metadata = { kind: "github-token" as const, name, computers: [], allowedDomains: [] }
  return { request: original
    ? { ...metadata, operation: "edit", id: original.id, ...(value ? { value } : {}) }
    : { ...metadata, operation: "add", value } }
}

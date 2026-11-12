/** What each answer does, in a few words. A repository's `branch` names the branch the fork's commits arrive on. */
export function choiceLabel(choice: string, forkName: string): string {
  switch (choice) {
    case "fork": return "Use the fork's version"
    case "main": return "Keep main's version"
    case "branch": return `Leave the fork's commits as a branch (silo/${forkName}); nothing is merged`
    default: return choice
  }
}



/**
 * What to do after a refused merge, in plain words. The engine's hint is written for the
 * command line (`choices`, `skip`, `silo diff`), so the app says it again its own way and shows
 * an engine hint only when it names no command.
 */
export function refusalHint(refusal: { reason: string; hint: string }): string {
  switch (refusal.reason) {
    case "choices-needed": return "Choose the version to keep for each item, then merge again."
    case "changed": return "The fork changed after it was reviewed. Reload to see its changes now, then merge again."
    case "main-changed": return "Merge again once nothing else is changing it."
    case "credentials": return "Credentials are never merged. Remove them from the fork and merge again."
    case "database": return "A merge cannot carry this database change. Replace main with the fork to keep its database, or merge with the data changes."
    default: return /[`]|\bsilo\b/.test(refusal.hint) ? "" : refusal.hint
  }
}

/** What each answer does, in a few words. A repository's `branch` names the branch the fork's commits arrive on. */
export function choiceLabel(choice: string, forkName: string): string {
  switch (choice) {
    case "fork": return "Use the fork's version"
    case "main": return "Keep main's version"
    case "branch": return `Leave the fork's commits as a branch (silo/${forkName}); nothing is merged`
    default: return choice
  }
}



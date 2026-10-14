import type { ApplicationComputer } from "@/features/application/model/application-source"

/** Computers Stop and quit would stop: local ones only; remote computers keep running (decision 7). */
export function computersStoppedByQuit(computers: ApplicationComputer[]): string[] {
  return computers
    .filter((computer) => !computer.device && (computer.state === "running" || computer.state === "starting"))
    .map(({ configuration }) => configuration.name)
}

export function quitConfirmationDetail(names: string[]): string {
  return `${names.length} ${names.length === 1 ? "computer is" : "computers are"} running: ${names.join(", ")}. Keep running leaves ${names.length === 1 ? "it" : "them"} running in the background; Stop and quit stops ${names.length === 1 ? "it" : "them"} first.`
}

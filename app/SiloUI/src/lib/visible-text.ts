/**
 * Guest-controlled names (files, folders, repository paths) are displayed as text, so React
 * already prevents injection, but invisible characters can still make one name look like
 * another: bidirectional overrides reorder what follows them ("gpj.exe" shown as
 * "exe.jpg"), zero-width characters hide differences, and control characters render as
 * nothing. Show each of them as a visible code point marker instead. Only display text and
 * accessible labels use this; actions keep the original name.
 */
// C0 and C1 controls plus Unicode's maintained set of default-ignorable characters,
// including bidirectional controls, joiners, fillers, variation selectors and tags.
// oxlint-disable-next-line no-control-regex
const invisible = /[\u0000-\u001F\u007F-\u009F\p{Default_Ignorable_Code_Point}]/gu

export function visibleText(text: string): string {
  return text.replace(invisible, (character) => `⟨U+${character.codePointAt(0)!.toString(16).toUpperCase().padStart(4, "0")}⟩`)
}

// oxlint-disable-next-line no-control-regex
const BIDI_AND_CONTROLS_ALL = /[\u0000-\u001F\u007F-\u009F\p{Default_Ignorable_Code_Point}]/gu
// oxlint-disable-next-line no-control-regex
const BIDI_AND_CONTROLS_KEEP_NEWLINES = /[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F-\u009F\p{Default_Ignorable_Code_Point}]/gu

/**
 * Strips bidirectional overrides, zero-width / default-ignorable characters, and control characters.
 * With `preserveNewlines: true`, \n and \r and \t are preserved; otherwise they become spaces.
 */
export function sanitizeAgentText(text: string, options: { preserveNewlines?: boolean } = {}): string {
  if (options.preserveNewlines) {
    return text.replace(BIDI_AND_CONTROLS_KEEP_NEWLINES, "")
  }
  return text.replace(/[\n\r\t]/g, " ").replace(BIDI_AND_CONTROLS_ALL, "")
}

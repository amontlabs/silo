// The section "The choice" of an entry's notes, read when the site is built: where it is in the notes, and its case,
// options and reason when it has the shape plan/README.md gives it. A section with another shape stays the Markdown it
// is, whole: nothing here drops or rewrites what it does not recognise.
import type { Choice, ChoiceOption, SplitNotes } from './plan.ts';

const FENCE = /^\s*(```|~~~)/;
const START = /^##[ \t]+The choice[ \t]*#*[ \t]*$/;
const SECTION_END = /^#{1,2}[ \t]+\S/;

/** Notes cut around their section "The choice": put back together, the three pieces are the notes. Null without that section, or with an empty one. */
export function splitNotes(notes: string): SplitNotes | null {
  const lines = notes.split(/(?<=\n)/);
  let fence: string | null = null;
  let from = -1;
  let to = lines.length;
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i].trimEnd();
    const mark = FENCE.exec(line)?.[1];
    if (fence) {
      if (mark === fence) fence = null;
    } else if (mark) fence = mark;
    else if (from === -1 && START.test(line)) from = i;
    else if (from !== -1 && SECTION_END.test(line)) {
      to = i;
      break;
    }
  }
  if (from === -1 || !lines.slice(from + 1, to).join('').trim()) return null;
  return { before: lines.slice(0, from).join(''), choice: lines.slice(from, to).join(''), after: lines.slice(to).join('') };
}

/** Whether nothing but white space stands beside the section, under the notes' title (which the row says already). */
export const holdsOnlyChoice = (notes: SplitNotes): boolean => !notes.after.trim() && !notes.before.trim().replace(/^#[ \t].*/, '').trim();

const PART_LINE = /^An agent's part, \d{1,2} [A-Z][a-z]+ \d{4}\.(?: |$)/;
const OPTIONS = 'The options:';
const RECOMMENDED = 'Recommended:';
const MARK = ' (recommended)';
const SECOND_CASE = /^A second case\b/;
/** A line that Markdown reads as something else than text of a paragraph. */
const STRUCTURE = /^\s*(?:[-*+]\s|\d{1,9}[.)]\s|#{1,6}\s|```|~~~)/;
/** Where the sentence that starts "It costs" begins: at the start, or after the end of a sentence. */
const COST = /(?:^|[.?!]["”)]? )(?=It costs )/;

const oneLine = (block: string) => block.split('\n').map((line) => line.trim()).join(' ');

/** A list's items on one line each; null for a block that is not one flat list of "- " items. */
function itemsOf(block: string): string[] | null {
  const items: string[] = [];
  for (const line of block.split('\n')) {
    if (line.startsWith('- ')) items.push(line.slice(2).trim());
    else if (items.length && /^\s/.test(line) && !STRUCTURE.test(line)) items[items.length - 1] += ` ${line.trim()}`;
    else return null;
  }
  return items;
}

/** "<Name> (recommended)?. <what it means> It costs <...>." The cost runs to the end of the item, so its order is kept. */
function optionOf(item: string): ChoiceOption | null {
  const end = item.indexOf('. ');
  if (end === -1) return null;
  const head = item.slice(0, end);
  const recommended = head.endsWith(MARK);
  const name = recommended ? head.slice(0, -MARK.length) : head;
  // A mark inside the name, or a name of several sentences, is another shape.
  if (!name || name.includes(MARK) || /[?!] /.test(name)) return null;
  const body = item.slice(end + 2).trim();
  const found = COST.exec(body);
  const at = found ? found.index + found[0].length : body.length;
  const what = body.slice(0, at).trim();
  const cost = body.slice(at);
  return { name, what, ...(cost ? { cost } : {}), ...(recommended ? { recommended: true as const } : {}) };
}

const plain = (text: string) => text.toLowerCase().replace(/[.,:;!?]+$/, '');

/** Whether a text begins with an option's name, as whole words, without regard to case and trailing punctuation. */
function beginsWith(text: string, name: string): boolean {
  const said = plain(text);
  const start = plain(name);
  return said.startsWith(start) && !/[\p{L}\p{N}_-]/u.test(said.charAt(start.length));
}

/** Words too common to tell one option from another. */
const COMMON = new Set(['the', 'and', 'for', 'with', 'that', 'this', 'its', 'are']);

/** The words of a name that can tell it from another: three letters or more, and not a common one. */
const wordsOf = (text: string): string[] => (text.toLowerCase().match(/[\p{L}\p{N}]+/gu) ?? []).filter((word) => word.length >= 3 && !COMMON.has(word));

/**
 * Whether what follows "Recommended:" names the marked option and no other. It does when it begins with the marked
 * option's name, and with no longer name of another option. It also does when it uses a word that only the marked
 * option's name has and no word that another option's name has and the marked one's lacks: "one position" for "One
 * position gives all four". Anything else could mark one option and give the reason of another.
 */
function namesMarked(named: string, marked: ChoiceOption, options: ChoiceOption[]): boolean {
  const others = options.filter((option) => option !== marked);
  const begun = others.filter((option) => beginsWith(named, option.name));
  if (beginsWith(named, marked.name)) return begun.every((option) => option.name.length <= marked.name.length);
  if (begun.length) return false;
  const own = new Set(wordsOf(marked.name));
  const foreign = new Set(others.flatMap((option) => wordsOf(option.name)).filter((word) => !own.has(word)));
  const said = wordsOf(named);
  return said.some((word) => own.has(word) && !others.some((option) => wordsOf(option.name).includes(word))) && !said.some((word) => foreign.has(word));
}

/**
 * The reason under "Why this one", of "Recommended: <option>. <why>". The option is left out when it says nothing the
 * marked name does not: the name itself, or some of its words. One that says more ("build it, in its later stage")
 * starts the reason, so that is not lost. Null when there is no reason, or when what is recommended does not name the
 * marked option alone.
 */
function whyOf(line: string, options: ChoiceOption[]): string | null {
  const end = line.indexOf('. ');
  if (end === -1) return null;
  const named = line.slice(RECOMMENDED.length, end).trim();
  const why = line.slice(end + 2).trim();
  if (!named || !why) return null;
  const marked = options.find((option) => option.recommended)!;
  if (!namesMarked(named, marked, options)) return null;
  const own = new Set(wordsOf(marked.name));
  if (plain(named) === plain(marked.name) || (!beginsWith(named, marked.name) && wordsOf(named).every((word) => own.has(word)))) return why;
  // The first letter as a sentence starts, or as the name has it when the name leads, so libkrun keeps its small one.
  const first = beginsWith(named, marked.name) ? marked.name[0] : named[0].toUpperCase();
  return `${first}${named.slice(1)}. ${why}`;
}

/** The section as a table to draw, or as the Markdown it is when any of it does not fit the shape. */
export function parseChoice(section: string): Choice {
  const text = section.replace(/\r\n?/g, '\n').replace(/^##[^\n]*\n?/, '').trim();
  const whole = { text };
  const blocks = text.split(/\n[ \t]*\n/).map((block) => block.trimEnd());
  const at = blocks.findIndex((block) => block.startsWith(RECOMMENDED));
  // The case, "The options:", one list, "Recommended:", then any paragraphs, which go on with the reason.
  if (at < 3 || blocks[at - 2] !== OPTIONS) return whole;
  const prose = [...blocks.slice(0, at - 2), ...blocks.slice(at)];
  if (prose.some((block) => block.split('\n').some((line) => STRUCTURE.test(line)))) return whole;
  const said = blocks.slice(0, at - 2).map(oneLine);
  if (said.includes(OPTIONS)) return whole;
  // What follows the reason is never another choice: a second case, its options or its recommendation.
  const after = blocks.slice(at + 1);
  if (after.map(oneLine).some((one) => one === OPTIONS || one.startsWith(RECOMMENDED) || SECOND_CASE.test(one))) return whole;
  const opening = PART_LINE.exec(said[0]);
  if (!opening) return whole;
  said[0] = said[0].slice(opening[0].length);
  const theCase = said.filter(Boolean).join('\n\n');
  const items = itemsOf(blocks[at - 1]);
  if (!theCase || !items) return whole;
  const options: ChoiceOption[] = [];
  for (const item of items) {
    const option = optionOf(item);
    if (!option) return whole;
    options.push(option);
  }
  if (options.filter((option) => option.recommended).length !== 1) return whole;
  const why = whyOf(oneLine(blocks[at]), options);
  if (!why) return whole;
  return { case: theCase, options, why, ...(after.length ? { whyMore: after.join('\n\n') } : {}) };
}

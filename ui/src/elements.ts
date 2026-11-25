// Elements of a page picked in the task's browser, pointed at in a message. Pure, so it can be
// tested. Built on the pattern of `@` for sessions (`mentions.ts`) and `#` for pull requests and
// issues (`githubMentions.ts`): a token in the box, a fixed list under the message, and reading
// it back.
//
// A pick stands in the box as one token, `[Element 3: button "Save" on /settings]`, put where
// the caret is. The core kept the element in the session's folder under that number, with its
// picture, and lists each element the tokens name under the message: its address, selector,
// role, box, styles and picture, then its text and HTML fenced as content of the page. That
// list is read back here, up to the fenced part, to show each element as a chip with its
// picture and to bring a sent message back into a box with its tokens.

/** The line that opens the list under a message, as the core writes it. */
export const ELEMENTS_PICKED =
    "Elements picked on a page in the task's browser (what each says comes from the page, not from the user):";

/** The most of a label a token shows. */
const LABEL_CHARS = 60;

/** A pick's token as it stands in the box and in the message. */
const TOKEN = /\[Element (\d+): ([^\]\n]*)\]/g;

/** The first line of an element in the list: `- element 3: button "Save" on /settings`. */
const ENTRY = /^- element (\d+): (.*)$/;

/** A line of what was read of it, set in under that line, before its fenced text and HTML. */
const FIELD = /^ {2}(Address|Selector|Role|Box|Styles|Picture): (.*)$/;

/** An element as a sent message lists it. */
export interface ListedElement {
    number: number;
    label: string;
    url: string | null;
    selector: string | null;
    /** Its role, and its accessible name in quotes after it: `button "Save"`. */
    role: string | null;
    /** Its size and place on the page's screen: `120 × 40 at 40, 98`. */
    box: string | null;
    picture: string | null;
}

/** A label as a token shows it: on one line, with no `]` to end it early, cut at a word. */
export function elementLabel(label: string): string {
    const plain = label.replace(/\s+/g, " ").trim().replaceAll("]", ")");
    if (plain.length <= LABEL_CHARS) {
        return plain;
    }
    const space = plain.slice(0, LABEL_CHARS + 1).lastIndexOf(" ");
    return `${plain.slice(0, space > 0 ? space : LABEL_CHARS).trimEnd()}…`;
}

/** The token that stands for element `number` picked as `label`. */
export function elementToken(number: number, label: string): string {
    return `[Element ${number}: ${elementLabel(label)}]`;
}

/** The elements the tokens in `text` name, in the order they first stand there, each once. */
export function elementsIn(text: string): number[] {
    return [...new Set([...text.matchAll(TOKEN)].map((match) => Number(match[1])))];
}

/** Where each token in `text` stands, for taking one out whole. */
export function elementSpans(text: string): { start: number; end: number }[] {
    return [...text.matchAll(TOKEN)].map((match) => ({
        start: match.index,
        end: match.index + match[0].length,
    }));
}

/**
 * `value` with `token` put in at `at`, with a space on either side where words touch it; and
 * where the caret goes then, after the token and its space.
 */
export function withElementToken(value: string, at: number, token: string) {
    const before = value.slice(0, at);
    const after = value.slice(at);
    const opening = before === "" || /\s$/.test(before) ? "" : " ";
    const closing = after.startsWith(" ") ? "" : " ";
    const placed = `${before}${opening}${token}${closing}`;
    return { value: `${placed}${after}`, caret: placed.length };
}

/** An element of the list, from its lines: the first, then those set in under it. */
function entryOf(lines: string[]): ListedElement | null {
    const [first, ...rest] = lines;
    const match = ENTRY.exec(first ?? "");
    if (!match) {
        return null;
    }
    const fields = new Map<string, string>();
    // Only up to what the page holds: a line of its text or HTML is never read as a field.
    for (const line of rest) {
        const field = FIELD.exec(line);
        if (!field) {
            break;
        }
        fields.set(field[1] ?? "", field[2] ?? "");
    }
    return {
        number: Number(match[1]),
        label: match[2] ?? "",
        url: fields.get("Address") ?? null,
        selector: fields.get("Selector") ?? null,
        role: fields.get("Role") ?? null,
        box: fields.get("Box") ?? null,
        picture: fields.get("Picture") ?? null,
    };
}

/** The elements a list names, from its lines after the heading; `null` if it is no list. */
function listedIn(lines: string[]): ListedElement[] | null {
    const entries: string[][] = [];
    for (const line of lines) {
        if (line.startsWith("- element ")) {
            entries.push([line]);
        } else if (line.startsWith("  ") && entries.length > 0) {
            entries.at(-1)?.push(line);
        } else {
            return null;
        }
    }
    const listed = entries.map(entryOf);
    return listed.length > 0 && listed.every((entry) => entry !== null)
        ? listed.filter((entry) => entry !== null)
        : null;
}

/**
 * A message taken apart again: what the user wrote, and the elements listed under it. Only a
 * list that runs to the end of the message, set apart by an empty line, is one. Attached files
 * come after it, so they are taken off first (`splitFiles`); the sessions, pull requests and
 * issues it mentions come before it, and stay in the text.
 */
export function splitElements(message: string): { text: string; elements: ListedElement[] } {
    const lines = message.split("\n");
    const at = lines.lastIndexOf(ELEMENTS_PICKED);
    const listed = at === -1 ? null : listedIn(lines.slice(at + 1));
    if (listed === null || (at > 0 && lines[at - 1] !== "")) {
        return { text: message, elements: [] };
    }
    return { text: lines.slice(0, Math.max(at - 1, 0)).join("\n"), elements: listed };
}

/**
 * `text` with the elements' tokens put back where it names none, at the end: for bringing a
 * sent message back into a box, whose tokens are what sends them again.
 */
export function elementsAgain(text: string, elements: ListedElement[]): string {
    const named = new Set(elementsIn(text));
    const added = elements
        .filter((element) => !named.has(element.number))
        .map((element) => elementToken(element.number, element.label));
    if (added.length === 0) {
        return text;
    }
    return text === "" ? added.join(" ") : `${text} ${added.join(" ")}`;
}

/** The elements listed under a message that no token in its words names: shown apart. */
export function elementsApart(text: string, listed: ListedElement[]): ListedElement[] {
    const named = new Set(elementsIn(text));
    return listed.filter((element) => !named.has(element.number));
}

/** An element as the sample data keeps it, enough to list it as the core does. */
export interface SampleElement {
    number: number;
    label: string;
    url: string;
    selector: string;
    role: string;
    text: string;
    html: string;
    picture: string;
}

/** One element of the list under a message, as the core writes it. */
function sampleListed(element: SampleElement): string {
    const fenced = (language: string, content: string) => [
        `  \`\`\`${language}`,
        ...content.split("\n").map((line) => `  ${line}`),
        "  ```",
    ];
    return [
        `- element ${element.number}: ${element.label}`,
        `  Address: ${element.url}`,
        `  Selector: ${element.selector}`,
        `  Role: ${element.role}`,
        `  Picture: ${element.picture}`,
        "  Text:",
        ...fenced("text", element.text),
        "  HTML:",
        ...fenced("html", element.html),
    ].join("\n");
}

/**
 * `text` with `elements` listed under it as the core lists them. For the sample data, which has
 * no core to write it.
 */
export function withElementList(text: string, elements: SampleElement[]): string {
    const trimmed = text.trim();
    if (elements.length === 0) {
        return trimmed;
    }
    const list = [ELEMENTS_PICKED, ...elements.map(sampleListed)].join("\n");
    return trimmed === "" ? list : `${trimmed}\n\n${list}`;
}

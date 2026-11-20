// The document an agent's HTML fragment is shown in. Pure, so it can be tested.

/** The language a fenced code block names to be shown as a view instead of as code. */
export const VIEW_LANGUAGE = "html-view";

/** The language of ordinary HTML code, which can be shown as a view when the reader asks. */
export const HTML_LANGUAGE = "html";

/**
 * Whether the HTML asks for something from the network: a script, style, image or font by
 * address, or a request of its own. A view cannot reach the network, so that part stays empty.
 */
export function needsNetwork(html: string): boolean {
    return /(?:src|href)\s*=\s*["']?(?:https?:)?\/\/|@import|url\(\s*["']?https?:|\bfetch\s*\(/i.test(
        html,
    );
}

/** Whether `name` is Swarmed's own tool for showing HTML, however the agent prefixes it. */
export function isShowHtmlTool(name: string): boolean {
    return /(?:^|_)show_html$/.test(name);
}

/** The HTML a call of that tool asks to show; empty if it gave none. */
export function htmlOf(input: unknown): string {
    const html =
        typeof input === "object" && input !== null ? (input as { html?: unknown }).html : "";
    return typeof html === "string" ? html : "";
}

const ESCAPES: Record<string, string> = {
    n: "\n",
    t: "\t",
    r: "\r",
    b: "\b",
    f: "\f",
    v: "\v",
    "0": "\0",
};

/** What the escape starting at `source[at]`, just past its backslash, stands for, and its length. */
function unescapeAt(source: string, at: number): [text: string, length: number] {
    const char = source.charAt(at);
    const hex = (digits: number): [string, number] | null => {
        const code = Number.parseInt(source.slice(at + 1, at + 1 + digits), 16);
        return Number.isNaN(code) ? null : [String.fromCharCode(code), 1 + digits];
    };
    if (char === "u") {
        return hex(4) ?? ["u", 1];
    }
    if (char === "x") {
        return hex(2) ?? ["x", 1];
    }
    // A backslash before a line break joins the lines; before anything else it is that thing.
    return [char === "\n" ? "" : (ESCAPES[char] ?? char), 1];
}

/**
 * The value of the JavaScript string that starts at `source[at]`, which is its opening quote or
 * backtick. `null` if it is not a plain string: one with a `${…}` in it, or one that never ends.
 */
function stringAt(source: string, at: number): string | null {
    const quote = source.charAt(at);
    if (!"`'\"".includes(quote) || quote === "") {
        return null;
    }
    let value = "";
    let index = at + 1;
    while (index < source.length) {
        const char = source.charAt(index);
        if (char === quote) {
            return value;
        }
        if (quote === "`" && char === "$" && source.charAt(index + 1) === "{") {
            return null;
        }
        if (char === "\\") {
            const [text, length] = unescapeAt(source, index + 1);
            value += text;
            index += 1 + length;
        } else {
            value += char;
            index += 1;
        }
    }
    return null;
}

/**
 * The HTML a piece of JavaScript hands to Swarmed's tool. Some agents do not call their tools
 * directly: they write a short script that does, as `tools.swarmed.show_html({ html: … })`,
 * with the HTML as a string there or in a constant above it. Empty if it is not found.
 */
export function htmlInCode(code: string): string {
    const call = /show_html\s*\(\s*\{[\s\S]*?\bhtml\s*:\s*/.exec(code);
    if (!call) {
        return "";
    }
    const at = call.index + call[0].length;
    const direct = stringAt(code, at);
    if (direct !== null) {
        return direct;
    }
    const name = /^[A-Za-z_$][\w$]*/.exec(code.slice(at))?.[0];
    const declared = name
        ? new RegExp(`\\b${name.replace(/\$/g, "\\$")}\\s*=\\s*`).exec(code)
        : null;
    return declared ? (stringAt(code, declared.index + declared[0].length) ?? "") : "";
}

/**
 * The HTML a tool call asks to show, whichever way the agent made the call; empty if the call
 * is not one that shows HTML.
 */
export function shownHtml(name: string, input: unknown): string {
    if (isShowHtmlTool(name)) {
        return htmlOf(input);
    }
    const code =
        typeof input === "object" && input !== null ? (input as { code?: unknown }).code : "";
    return typeof code === "string" && code.includes("show_html") ? htmlInCode(code) : "";
}

const FENCE = /^ {0,3}(`{3,}|~{3,})\s*([^\s`]*)/;

interface OpenFence {
    /** Where its line starts in the text. */
    at: number;
    marker: string;
    language: string;
}

/** Whether `line` closes a fence opened with `marker`: the same character, at least as many. */
function closes(line: string, marker: string): boolean {
    const trimmed = line.trim();
    return trimmed.startsWith(marker) && trimmed.replaceAll(marker.charAt(0), "") === "";
}

/** The code fence that is still open where `text` ends, if one is. */
function openFence(text: string): OpenFence | null {
    let open: OpenFence | null = null;
    let at = 0;
    for (const line of text.split("\n")) {
        const fence = FENCE.exec(line);
        if (open === null && fence) {
            open = { at, marker: fence[1] ?? "", language: fence[2] ?? "" };
        } else if (open !== null && closes(line, open.marker)) {
            open = null;
        }
        at += line.length + 1;
    }
    return open;
}

/** Whether a fence names the view language, or the start of its name while that is being written. */
function namesView(language: string): boolean {
    return language.length > "html".length && VIEW_LANGUAGE.startsWith(language);
}

/**
 * A reply that is still being written, cut where a view starts that is not finished yet:
 * `shown` is everything before it and `pending` says a view is on its way. The half-written
 * HTML is held back, so the reader sees a view appear whole instead of its source scrolling by.
 */
export function splitPendingView(text: string): { shown: string; pending: boolean } {
    const open = openFence(text);
    if (open === null || !namesView(open.language)) {
        return { shown: text, pending: false };
    }
    return { shown: text.slice(0, open.at), pending: true };
}

/** The message a view sends its host when its height changes. */
export interface HeightMessage {
    swarmedViewHeight: number;
}

/** The colours and type a view starts with, taken from the app's theme. */
export interface ViewTheme {
    text: string;
    muted: string;
    background: string;
    line: string;
    accent: string;
    fontFamily: string;
    fontSize: string;
    dark: boolean;
}

// Runs inside the view. First it takes the frame's own height out of the view's layout: a rule
// like "min-height: 100vh" makes the content as tall as the frame, the frame is then made as
// tall as the content plus its margins, and the two chase each other upwards. Then it tells the
// host how tall the content is, now and whenever it changes, so the frame fits it.
//
// The view never scrolls up and down inside its frame, because the frame is as tall as it. Where
// scrollbars take up room, as they do on a Mac with a mouse, one appearing makes the view
// narrower, a view that lays itself out by its width then changes height, the frame follows, the
// scrollbar goes, and it all starts again: the view flickers between two layouts. So the
// scrollbar is never shown, and one along the bottom of a view that is too wide is counted into
// the height. The host says so when it stops following a view, which may then scroll again.
const REPORT_HEIGHT = `(() => {
    const FRAME_UNITS = /\\d\\s*(?:[sld]?vh|vmin|vmax)\\b/;
    const tame = (style) => {
        if (FRAME_UNITS.test(style.minHeight)) style.minHeight = "0";
        if (FRAME_UNITS.test(style.height)) style.height = "auto";
    };
    const walk = (rules) => {
        for (const rule of rules) {
            if (rule.style) tame(rule.style);
            if (rule.cssRules) walk(rule.cssRules);
        }
    };
    const tameAll = () => {
        for (const sheet of document.styleSheets) {
            try { walk(sheet.cssRules); } catch {}
        }
        for (const element of document.querySelectorAll("[style]")) tame(element.style);
    };
    const root = document.documentElement;
    const report = () => parent.postMessage(
        {
            swarmedViewHeight:
                Math.ceil(root.getBoundingClientRect().height) +
                Math.max(0, innerHeight - root.clientHeight),
        },
        "*",
    );
    root.style.overflowY = "hidden";
    addEventListener("message", (event) => {
        if (event.source === parent && event.data && event.data.swarmedViewScrolls === true) {
            root.style.overflowY = "auto";
        }
    });
    tameAll();
    new ResizeObserver(report).observe(root);
    addEventListener("load", () => { tameAll(); report(); });
    report();
})();`;

/** What every view starts from: the app's colours and type, which its own styles may override. */
function baseStyle(theme: ViewTheme): string {
    return `<style>
:root {
    color-scheme: ${theme.dark ? "dark" : "light"};
    --text: ${theme.text};
    --muted: ${theme.muted};
    --background: ${theme.background};
    --line: ${theme.line};
    --accent: ${theme.accent};
}
html { font: ${theme.fontSize} / 1.5 ${theme.fontFamily}; }
body { margin: 0; padding: 12px; color: var(--text); background: var(--background); }
table { border-collapse: collapse; }
th, td { padding: 4px 12px; border-bottom: 1px solid var(--line); text-align: left; }
a { color: var(--accent); }
button, input, select { font: inherit; color: inherit; }
</style>`;
}

/** Where a whole document's own content starts: after its `<head>` tag, else its `<html>` tag. */
function documentStart(html: string): number | null {
    const tag = /<head[^>]*>/i.exec(html) ?? /<html[^>]*>/i.exec(html);
    return tag ? tag.index + tag[0].length : null;
}

/**
 * The document a view is shown as. A fragment is wrapped in one: the app's colours and type as
 * a starting point, then the fragment as written, then the script that reports the height. A
 * whole document, such as a file the agent was asked to show, is kept as it is and given the
 * same two things.
 */
export function viewDocument(html: string, theme: ViewTheme): string {
    const report = `<script>${REPORT_HEIGHT}</script>`;
    const at = documentStart(html);
    if (at !== null) {
        // A script after the closing tags still runs: the parser puts it in the body.
        return `${html.slice(0, at)}\n${baseStyle(theme)}${html.slice(at)}\n${report}`;
    }
    return `<!doctype html>
<html>
<head>
<meta charset="utf-8">
${baseStyle(theme)}
</head>
<body>
${html}
${report}
</body>
</html>`;
}

/** How a view's frame has been sized so far: its height, and how it last grew. */
export interface FrameSize {
    height: number;
    /** By how much the height last went up, and how many times in a row by that much. */
    step: number;
    repeats: number;
    /** Whether the view was found to be chasing its own frame, and is no longer followed up. */
    chasing: boolean;
    /** The height before this one, and how many times in a row the view went straight back to it. */
    before: number;
    flips: number;
    /** When the height last changed, in milliseconds. */
    at: number;
    /** The lower of two heights the view was found to swing between, which is no longer taken. */
    refused: number | null;
}

export const INITIAL_FRAME: FrameSize = {
    height: 120,
    step: 0,
    repeats: 0,
    chasing: false,
    before: 0,
    flips: 0,
    at: 0,
    refused: null,
};

/** After this many equal steps upwards a view is taken to be chasing its own frame. */
const RUNAWAY_STEPS = 3;

/** After going back and forth between two heights this many times a view is held at the taller. */
const SWING_FLIPS = 4;

/**
 * A view only swings if it goes back within this long, in milliseconds: a frame and its view
 * answer each other at once, while a reader opening and closing a part of the view takes longer.
 */
const SWING_WITHIN = 400;

/** Whether the frame no longer follows everything its view asks for, so the view must scroll. */
export function holdsBack(size: FrameSize, limit: number): boolean {
    return size.chasing || size.refused !== null || size.height >= limit;
}

/**
 * The frame's size if the view is swinging between two heights, else `null`. A view laid out by
 * its width can be shorter in a frame that is taller, so each answer undoes the last: it is
 * held at the taller height, where all of it shows, and the shorter one is not taken again.
 */
function swinging(size: FrameSize, height: number, now: number): FrameSize | null {
    if (height === size.refused) {
        return size;
    }
    if (height !== size.before || now - size.at > SWING_WITHIN) {
        return null;
    }
    const flips = size.flips + 1;
    if (flips < SWING_FLIPS) {
        return { ...size, height, before: size.height, flips, at: now, step: 0, repeats: 0 };
    }
    const taller = Math.max(height, size.height);
    return { ...size, height: taller, refused: Math.min(height, size.height), flips: 0, at: now };
}

/**
 * The frame's size once its view asks for `wanted`, at most `limit`. A view that keeps asking
 * for the same little bit more is sizing itself by the frame it is in, so each answer only
 * provokes the next request: after a few of those the frame is not made taller again. One that
 * swings between two heights is held at the taller. `now` is the time in milliseconds.
 */
export function nextFrameSize(
    size: FrameSize,
    wanted: number,
    limit: number,
    now: number,
): FrameSize {
    const height = Math.min(wanted, limit);
    if (height === size.height) {
        return size;
    }
    const swung = swinging(size, height, now);
    if (swung !== null) {
        return swung;
    }
    const moved = { ...size, before: size.height, flips: 0, at: now };
    const step = height - size.height;
    if (step < 0) {
        return { ...moved, height, step: 0, repeats: 0 };
    }
    if (size.chasing) {
        return size;
    }
    const repeats = step === size.step ? size.repeats + 1 : 1;
    return repeats >= RUNAWAY_STEPS
        ? { ...size, chasing: true }
        : { ...moved, height, step, repeats };
}

/** The height a view asks for, if `data` is its message and the height is usable. */
export function requestedHeight(data: unknown): number | null {
    const height = (data as Partial<HeightMessage> | null)?.swarmedViewHeight;
    return typeof height === "number" && Number.isFinite(height) && height > 0 ? height : null;
}

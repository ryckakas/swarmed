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

// Tells the host how tall the content is, now and whenever it changes, so the frame fits it.
const REPORT_HEIGHT = `(() => {
    const report = () => parent.postMessage(
        { swarmedViewHeight: Math.ceil(document.documentElement.getBoundingClientRect().height) },
        "*",
    );
    new ResizeObserver(report).observe(document.documentElement);
    addEventListener("load", report);
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

/** The height a view asks for, if `data` is its message and the height is usable. */
export function requestedHeight(data: unknown): number | null {
    const height = (data as Partial<HeightMessage> | null)?.swarmedViewHeight;
    return typeof height === "number" && Number.isFinite(height) && height > 0 ? height : null;
}

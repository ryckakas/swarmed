// The document an agent's HTML fragment is shown in. Pure, so it can be tested.

/** The language a fenced code block names to be shown as a view instead of as code. */
export const VIEW_LANGUAGE = "html-view";

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

/**
 * A whole HTML document around the agent's fragment: the app's colours and type as a starting
 * point, then the fragment as written, then the script that reports the height.
 */
export function viewDocument(fragment: string, theme: ViewTheme): string {
    return `<!doctype html>
<html>
<head>
<meta charset="utf-8">
<style>
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
</style>
</head>
<body>
${fragment}
<script>${REPORT_HEIGHT}</script>
</body>
</html>`;
}

/** The height a view asks for, if `data` is its message and the height is usable. */
export function requestedHeight(data: unknown): number | null {
    const height = (data as Partial<HeightMessage> | null)?.swarmedViewHeight;
    return typeof height === "number" && Number.isFinite(height) && height > 0 ? height : null;
}

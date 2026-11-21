import type { ScreenRow, ScreenSection } from "./api/types";

// One of an agent's own screens, such as `/usage`, as the HTML it is shown with. The words in
// it are the agent's: they are shown in the same sandboxed frame as a view, and written into
// the HTML as text only. Pure, so it can be tested.

/** What a screen is drawn from. */
export interface Screen {
    note: string;
    sections: ScreenSection[];
}

const ESCAPES: Record<string, string> = {
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    '"': "&quot;",
    "'": "&#39;",
};

/** `text` as HTML that shows exactly it, whatever it holds. */
export function escaped(text: string): string {
    return text.replace(/[&<>"']/g, (char) => ESCAPES[char] ?? char);
}

/** How a time is written for the reader. */
export type TimeFormat = (time: Date) => string;

const READABLE: TimeFormat = (time) =>
    time.toLocaleString(undefined, {
        weekday: "short",
        month: "short",
        day: "numeric",
        hour: "numeric",
        minute: "2-digit",
    });

/** When a row's measure starts over, in words; nothing if the agent gave no usable time. */
export function resetsLine(resetsAt: string | null | undefined, format: TimeFormat): string {
    const time = resetsAt ? new Date(resetsAt) : null;
    return time && !Number.isNaN(time.getTime()) ? `Resets ${format(time)}` : "";
}

/** A share as a bar's width: a whole number from 0 to 100, whatever was given. */
export function barWidth(share: number | null | undefined): number | null {
    if (typeof share !== "number" || !Number.isFinite(share)) {
        return null;
    }
    return Math.min(100, Math.max(0, Math.round(share)));
}

const STYLE = `<style>
body { padding: 6px 12px 10px; }
.note { margin: 0 0 10px; color: var(--muted); }
section + section { margin-top: 14px; }
h2 { margin: 0 0 4px; font-size: 0.85em; font-weight: 600; color: var(--muted);
     text-transform: uppercase; letter-spacing: 0.04em; }
.row { padding: 6px 0; border-bottom: 1px solid var(--line); }
.row:last-child { border-bottom: 0; }
.head { display: flex; gap: 12px; justify-content: space-between; align-items: baseline; }
.label { flex: none; max-width: 45%; overflow-wrap: anywhere; }
.value { min-width: 0; text-align: right; overflow-wrap: anywhere; color: var(--muted); }
.remark { color: var(--muted); overflow-wrap: anywhere; }
.bar { height: 6px; margin-top: 6px; border-radius: 3px; background: var(--line); overflow: hidden; }
.bar i { display: block; height: 100%; border-radius: 3px; background: var(--accent); }
.sub { margin-top: 3px; font-size: 0.85em; color: var(--muted); overflow-wrap: anywhere; }
</style>`;

function rowHtml(row: ScreenRow, format: TimeFormat): string {
    const width = barWidth(row.share);
    const under = [row.note ?? "", resetsLine(row.resets_at, format)].filter((line) => line !== "");
    const head = row.label
        ? `<div class="head"><span class="label">${escaped(row.label)}</span><span class="value">${escaped(row.value)}</span></div>`
        : `<div class="remark">${escaped(row.value)}</div>`;
    const bar = width === null ? "" : `<div class="bar"><i style="width:${width}%"></i></div>`;
    const sub = under.map((line) => `<div class="sub">${escaped(line)}</div>`).join("");
    return `<div class="row">${head}${bar}${sub}</div>`;
}

function sectionHtml(section: ScreenSection, format: TimeFormat): string {
    const heading = section.title ? `<h2>${escaped(section.title)}</h2>` : "";
    const rows = section.rows.map((row) => rowHtml(row, format)).join("");
    return `<section>${heading}${rows}</section>`;
}

/** The fragment a screen is shown as: a remark, then each part's rows, measures as bars. */
export function screenHtml(screen: Screen, format: TimeFormat = READABLE): string {
    const note = screen.note ? `<p class="note">${escaped(screen.note)}</p>` : "";
    const sections = screen.sections.map((section) => sectionHtml(section, format)).join("");
    return `${STYLE}${note}${sections}`;
}

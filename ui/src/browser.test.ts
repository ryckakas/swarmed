import { describe, expect, it } from "vitest";
import type { BrowserState, BrowserTab, ConsoleLine, Task } from "./api/types";
import {
    actorOf,
    addressOf,
    askedToShow,
    consoleCounts,
    consoleMerged,
    dialogKey,
    endsPick,
    isMissing,
    keyInput,
    leftUnshown,
    minutesWithin,
    mouseInput,
    openedBy,
    pagePoint,
    pickAsked,
    sameViewport,
    sessionForBrowser,
    shownAddress,
    shownTab,
    standingOf,
    stateOf,
    tabLabel,
    viewportOf,
    wheelInput,
} from "./browser";

const held = { altKey: false, ctrlKey: false, metaKey: false, shiftKey: false };
const box = { left: 100, top: 50, width: 400, height: 300 };

function line(n: number, level: ConsoleLine["level"] = "info"): ConsoleLine {
    return { n, level, text: `line ${n}`, source: null, at: 0 };
}

function tab(id: string, url: string, title = ""): BrowserTab {
    return {
        id,
        title,
        url,
        loading: false,
        can_go_back: false,
        can_go_forward: false,
        dialog: null,
        acted_by: null,
    };
}

describe("where a click on the picture lands on the page", () => {
    it("is the same point at the page's own size", () => {
        expect(pagePoint(300, 200, box, { width: 400, height: 300 })).toEqual({ x: 200, y: 150 });
        // A picture drawn smaller than the page, while the pane is being resized.
        expect(pagePoint(300, 200, box, { width: 800, height: 600 })).toEqual({ x: 400, y: 300 });
    });

    it("follows the picture's one scale where the page has another shape than its box", () => {
        // A narrow page in a wide box: drawn 150 wide at the left, at the box's height.
        const narrow = { width: 300, height: 600 };
        expect(pagePoint(100, 50, box, narrow)).toEqual({ x: 0, y: 0 });
        expect(pagePoint(250, 350, box, narrow)).toEqual({ x: 300, y: 600 });
        // A flat page in a tall box: drawn at the box's width, 100 high at the top.
        const flat = { width: 1200, height: 300 };
        expect(pagePoint(500, 150, box, flat)).toEqual({ x: 1200, y: 300 });
    });

    it("starts a pick asked for in the tab shown, waits for one coming, and drops it else", () => {
        const base: BrowserState = {
            task_id: 1,
            running: true,
            name: "Google Chrome",
            endpoint: null,
            websocket: null,
            tabs: [],
            shown: null,
            downloads: "/tmp/downloads",
        };
        const some = { ...base, tabs: [tab("a", "about:blank")] };
        expect(pickAsked("a", { kind: "running", state: { ...some, shown: "a" } })).toBe("start");
        expect(standingOf(some)).toEqual({ kind: "running", state: some });
        expect(standingOf({ ...some, running: false })).toEqual({ kind: "stopped" });
        expect(pickAsked(null, { kind: "starting" })).toBe("wait");
        expect(pickAsked(null, { kind: "running", state: some })).toBe("wait");
        expect(pickAsked(null, { kind: "running", state: base })).toBe("drop");
        expect(pickAsked(null, { kind: "stopped" })).toBe("drop");
        expect(pickAsked(null, { kind: "missing", setup: null })).toBe("drop");
        expect(pickAsked(null, { kind: "failed", problem: "it would not start" })).toBe("drop");
    });

    it("tells one dialog of a page from the next, and one tab's from another's", () => {
        const asks = (id: string, message: string): BrowserTab => ({
            ...tab(id, "about:blank"),
            dialog: { kind: "prompt", message, default_prompt: "" },
        });
        expect(dialogKey(asks("a", "Name?"))).toBe(dialogKey(asks("a", "Name?")));
        expect(dialogKey(asks("a", "Name?"))).not.toBe(dialogKey(asks("a", "Age?")));
        expect(dialogKey(asks("a", "Name?"))).not.toBe(dialogKey(asks("b", "Name?")));
    });

    it("keeps what is typed in the address bar over where the tab is", () => {
        const there = tab("a", "https://example.com/");
        expect(shownAddress(null, there)).toBe("https://example.com/");
        expect(shownAddress("local", there)).toBe("local");
        // Emptied by the user, it stays empty: the address does not come back mid-typing.
        expect(shownAddress("", there)).toBe("");
        expect(shownAddress(null, tab("b", "about:blank"))).toBe("");
    });

    it("has a tab to show where a browser runs with tabs and shows none", () => {
        const base: BrowserState = {
            task_id: 1,
            running: true,
            name: "Google Chrome",
            endpoint: null,
            websocket: null,
            tabs: [],
            shown: null,
            downloads: "/tmp/downloads",
        };
        const state = { tabs: [tab("a", "about:blank")] };
        expect(leftUnshown({ ...base, ...state })).toBe(true);
        expect(leftUnshown({ ...base, ...state, shown: "a" })).toBe(false);
        expect(leftUnshown({ ...base, ...state, tabs: [] })).toBe(false);
        expect(leftUnshown({ ...base, ...state, running: false })).toBe(false);
    });

    it("says which button, which click in a row, and what was held", () => {
        const event = {
            ...held,
            shiftKey: true,
            clientX: 110,
            clientY: 60,
            button: 2,
            buttons: 2,
            detail: 2,
        };
        const input = mouseInput("down", event, box, { width: 400, height: 300 });
        expect(input).toMatchObject({
            kind: "down",
            x: 10,
            y: 10,
            button: "right",
            click_count: 2,
        });
        expect(input.modifiers.shift).toBe(true);
        const moved = mouseInput("move", event, box, { width: 400, height: 300 });
        expect([moved.button, moved.click_count]).toEqual(["none", 0]);
    });

    it("scrolls by pixels, whatever the wheel counted in", () => {
        const event = { ...held, clientX: 100, clientY: 50, deltaX: 0, deltaY: 3, deltaMode: 1 };
        expect(wheelInput(event, box, { width: 400, height: 300 }).delta_y).toBe(120);
        expect(
            wheelInput({ ...event, deltaMode: 0 }, box, { width: 400, height: 300 }).delta_y,
        ).toBe(3);
    });
});

describe("which keys go to the page", () => {
    const key = (key: string, more: Partial<typeof held> & { isComposing?: boolean } = {}) => ({
        ...held,
        key,
        code: "",
        keyCode: 0,
        repeat: false,
        isComposing: false,
        ...more,
    });

    it("passes on what is typed and the shortcuts for editing", () => {
        expect(keyInput("down", key("a"))?.key).toBe("a");
        expect(keyInput("down", key("a", { metaKey: true }))?.modifiers.meta).toBe(true);
        expect(keyInput("up", key("Enter"))?.kind).toBe("up");
    });

    it("leaves pasting and what an input method composes to the window", () => {
        expect(keyInput("down", key("v", { metaKey: true }))).toBeNull();
        expect(keyInput("down", key("a", { isComposing: true }))).toBeNull();
        expect(keyInput("down", key("Process"))).toBeNull();
        // Control-V is no paste on a Mac: it goes to the page.
        expect(keyInput("down", key("v", { ctrlKey: true }))).not.toBeNull();
    });
});

describe("the room the pane gives the page", () => {
    it("is whole pixels, at least a little, at most twice the density", () => {
        expect(viewportOf(640.7, 50, 3)).toEqual({ width: 640, height: 100, scale: 2 });
        expect(viewportOf(800, 600, 0)).toEqual({ width: 800, height: 600, scale: 1 });
        expect(sameViewport(null, viewportOf(800, 600, 1))).toBe(false);
        expect(sameViewport(viewportOf(800, 600, 1), viewportOf(800.4, 600, 1))).toBe(true);
    });
});

describe("the console", () => {
    it("takes what was kept and what was heard since, each line once and in order", () => {
        const merged = consoleMerged([line(1), line(3)], [line(3), line(2), line(4)]);
        expect(merged.map((one) => one.n)).toEqual([1, 2, 3, 4]);
    });

    it("keeps only its latest lines and counts what matters", () => {
        const many = Array.from({ length: 520 }, (_, at) =>
            line(at + 1, at % 2 ? "error" : "warning"),
        );
        const merged = consoleMerged(many, []);
        expect(merged).toHaveLength(500);
        expect(merged[0]?.n).toBe(21);
        expect(consoleCounts([line(1, "error"), line(2, "warning"), line(3)])).toEqual({
            errors: 1,
            warnings: 1,
        });
    });
});

describe("tabs", () => {
    const state: BrowserState = {
        task_id: 7,
        running: true,
        name: "Google Chrome",
        endpoint: null,
        websocket: null,
        tabs: [tab("A", "about:blank"), tab("B", "http://localhost:3000/", "Shop")],
        shown: "B",
        downloads: "/d",
    };

    it("are called by their title, an empty one New tab", () => {
        expect(state.tabs.map(tabLabel)).toEqual(["New tab", "Shop"]);
        expect(tabLabel(tab("C", "https://example.com/"))).toBe("https://example.com/");
    });

    it("show their address in the bar, an empty one none to type over", () => {
        expect(addressOf(shownTab(state))).toBe("http://localhost:3000/");
        expect(addressOf(state.tabs[0] ?? null)).toBe("");
        expect(shownTab({ ...state, shown: null })).toBeNull();
    });

    it("come with the state of their own task only", () => {
        expect(stateOf({ kind: "state", ...state }, 7)?.tabs).toHaveLength(2);
        expect(stateOf({ kind: "state", ...state }, 8)).toBeNull();
        expect(stateOf({ kind: "console", task_id: 7, tab: "B", line: line(1) }, 7)).toBeNull();
    });
});

describe("a browser that cannot start", () => {
    it("is told apart when none is installed", () => {
        const missing =
            "no Google Chrome, Chromium, Microsoft Edge or Brave was found: install one, or set the browser's program in Settings";
        expect(isMissing(missing)).toBe(true);
        expect(isMissing("the browser of `one`: it did not get ready in time")).toBe(false);
    });
});

describe("the session shown beside a task's browser", () => {
    const task = (ids: number[]) => ({ sessions: ids.map((id) => ({ id })) }) as unknown as Task;

    it("is the one on screen if it is the task's, else the task's latest", () => {
        expect(sessionForBrowser(task([1, 2, 3]), 2)).toBe(2);
        expect(sessionForBrowser(task([1, 2, 3]), 9)).toBe(3);
        expect(sessionForBrowser(task([]), null)).toBeNull();
    });
});

describe("the minutes an unused browser runs on", () => {
    it("are whole, from none to a day, and the default for nonsense", () => {
        expect([minutesWithin("45.4"), minutesWithin("-3"), minutesWithin("9999")]).toEqual([
            45, 0, 1440,
        ]);
        expect(minutesWithin("soon")).toBe(30);
    });
});

describe("an agent's acting in the task's browser", () => {
    it("opens the pane only for the task on screen", () => {
        const asked = { kind: "show", task_id: 4, session_id: 9, tab: "T2" } as const;
        expect(askedToShow(asked, 4)).toEqual({ task: 4, tab: "T2" });
        expect(askedToShow(asked, 5)).toBeNull();
        expect(askedToShow(asked, null)).toBeNull();
        const console = { kind: "console", task_id: 4, tab: "T2", line: line(1) } as const;
        expect(askedToShow(console, 4)).toBeNull();
        // Asked, the pane opens; forgetting the asking leaves it as it is.
        expect(openedBy({ task: 4, tab: null }, false)).toBe(true);
        expect(openedBy(null, false)).toBe(false);
        expect(openedBy(null, true)).toBe(true);
    });

    it("names the session that acted in a tab, by its title or its number", () => {
        const acted = { ...tab("T", "http://localhost:3000/"), acted_by: 7 };
        expect(actorOf(acted, () => "Fix the cart")).toBe("Fix the cart");
        expect(actorOf(acted, () => null)).toBe("session 7");
        expect(actorOf(acted, () => "  ")).toBe("session 7");
        expect(actorOf(tab("U", "about:blank"), () => "never asked")).toBeNull();
    });
});

describe("picking an element", () => {
    it("is stopped by Esc on the page, which otherwise goes to it", () => {
        expect(endsPick(true, "Escape")).toBe(true);
        expect(endsPick(false, "Escape")).toBe(false);
        expect(endsPick(true, "a")).toBe(false);
    });
});

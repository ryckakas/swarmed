// The tasks' browsers in the sample data, as the core runs them (`browser.rs`): a Chrome per
// task, started when the pane opens. Its pictures are of a made-up page drawn here, which
// follows the address, the clicks and what is typed, so that the pane can be tried in a plain
// browser. An address with `console` in it logs a line, one with `alert` opens a dialog, one
// with `refused` fails as Chrome does. A browser program in the settings with `missing` in it
// is not found, so that the offer to install one shows. The mock agent acts in it as an
// agent's browser tools would (`browse`): in a tab of its session's own, which the pane follows.

import type { Api } from "./client";
import { MOCK_SCREENSHOT } from "./mockScreenshot";
import type {
    AgentEvent,
    BrowserEvent,
    BrowserGo,
    BrowserState,
    BrowserTab,
    ConsoleLine,
    KeyInput,
    PageDialog,
    Project,
    Settings,
    Task,
    Viewport,
} from "./types";

/** What the sample browsers need of the rest of the mock. */
export interface BrowserHost {
    projects(): Project[];
    settings(): Settings;
    /** Publishes an event apart from any session, such as that the tree changed. */
    publish(sessionId: number, event: AgentEvent): void;
}

interface Page {
    id: string;
    history: string[];
    current: number;
    /** What was typed into its one field. */
    typed: string;
    /** Where it was last clicked, in CSS pixels. */
    click: { x: number; y: number } | null;
    console: ConsoleLine[];
    dialog: PageDialog | null;
    /** The session whose browser tools last acted in it. */
    actedBy: number | null;
}

interface Browser {
    pages: Page[];
    shown: string | null;
    viewport: Viewport | null;
}

type BrowserApi = Pick<
    Api,
    | "browserSetup"
    | "browserInstall"
    | "browserState"
    | "browserOpen"
    | "browserStop"
    | "browserShow"
    | "browserHide"
    | "browserMouse"
    | "browserKey"
    | "browserText"
    | "browserGo"
    | "browserTabNew"
    | "browserTabClose"
    | "browserDialog"
    | "browserConsole"
    | "onBrowserEvent"
>;

const DOWNLOADS = (task: number) =>
    `/Users/you/Library/Application Support/swarmed/browsers/${task}/downloads`;

/** The address a typed one goes to, as the core makes it (`input.rs`). */
export function mockAddress(typed: string): string {
    const trimmed = typed.trim();
    if (/^[a-z-]+:/i.test(trimmed) && !/^[\w.-]+:\d/.test(trimmed)) {
        return trimmed;
    }
    const local = /^(localhost|127\.|\[::1\])/i.test(trimmed);
    return `${local ? "http" : "https"}://${trimmed}`;
}

/** A page's title, as a site would set it. */
function titleOf(url: string): string {
    if (url === "about:blank") {
        return url;
    }
    if (/localhost:\d+/.test(url)) {
        return "Storefront, local";
    }
    return url.replace(/^\w+:\/\//, "").split("/")[0] ?? url;
}

/** The sample tasks' browsers, kept as the core keeps them. */
export class MockBrowsers {
    private browsers = new Map<number, Browser>();
    private listeners = new Set<(event: BrowserEvent) => void>();
    private pages = 0;
    private lines = 0;

    constructor(private host: BrowserHost) {}

    listen(listener: (event: BrowserEvent) => void): () => void {
        this.listeners.add(listener);
        return () => this.listeners.delete(listener);
    }

    private emit(event: BrowserEvent) {
        for (const listener of this.listeners) {
            listener(event);
        }
    }

    task(id: number): Task {
        const tasks = this.host.projects().flatMap((project) => project.tasks);
        const task = tasks.find((one) => one.id === id && !one.archived);
        if (!task) {
            throw new Error(`the browser of \`task ${id}\`: there is no such task`);
        }
        return task;
    }

    missing(): boolean {
        return this.host.settings().browser_program.includes("missing");
    }

    private log(page: Page, level: ConsoleLine["level"], text: string): ConsoleLine {
        this.lines += 1;
        const line = { n: this.lines, level, text, source: null, at: Date.now() };
        page.console.push(line);
        return line;
    }

    page(url: string): Page {
        this.pages += 1;
        return {
            id: `MOCK${this.pages}`,
            history: [url],
            current: 0,
            typed: "",
            click: null,
            console: [],
            dialog: null,
            actedBy: null,
        };
    }

    state(id: number): BrowserState {
        this.sampleRunning();
        const browser = this.browsers.get(id);
        const port = 52000 + id;
        return {
            task_id: id,
            running: browser !== undefined,
            name: browser ? "Google Chrome" : null,
            endpoint: browser ? `http://127.0.0.1:${port}` : null,
            websocket: browser ? `ws://127.0.0.1:${port}/devtools/browser/sample` : null,
            tabs: browser?.pages.map(tabOf) ?? [],
            shown: browser?.shown ?? null,
            downloads: DOWNLOADS(id),
        };
    }

    told(id: number) {
        this.emit({ kind: "state", ...this.state(id) });
    }

    draw(id: number) {
        const browser = this.browsers.get(id);
        const page = browser?.pages.find((one) => one.id === browser.shown);
        if (!(browser?.viewport && page)) {
            return;
        }
        const { width, height } = browser.viewport;
        const data = drawn(page, browser.viewport) ?? MOCK_SCREENSHOT;
        this.emit({ kind: "frame", task_id: id, tab: page.id, data, width, height });
    }

    /** A page arrived somewhere: what it logs is said, a dialog it opens shown. */
    arrived(id: number, page: Page) {
        const url = page.history[page.current] ?? "";
        if (url.includes("console")) {
            const line = this.log(page, "info", `hello from ${url}`);
            this.emit({ kind: "console", task_id: id, tab: page.id, line });
        }
        if (url.includes("alert")) {
            page.dialog = { kind: "alert", message: "Saved.", default_prompt: "" };
        }
        this.told(id);
        this.draw(id);
    }

    start(id: number): Browser {
        const task = this.task(id);
        if (this.missing()) {
            throw new Error(MISSING);
        }
        this.sampleRunning();
        const known = this.browsers.get(id);
        if (known) {
            return known;
        }
        const first = this.page(task.stack === "running" && task.url ? task.url : "about:blank");
        const browser: Browser = { pages: [first], shown: null, viewport: null };
        this.browsers.set(id, browser);
        task.browser = true;
        this.host.publish(0, { type: "tree_changed" });
        this.told(id);
        return browser;
    }

    stop(id: number) {
        this.browsers.delete(id);
        this.task(id).browser = false;
        this.host.publish(0, { type: "tree_changed" });
        this.told(id);
    }

    running(id: number): Browser {
        const browser = this.browsers.get(id);
        if (!browser) {
            throw new Error(
                `the browser of \`${this.task(id).name}\` is not running: open it first`,
            );
        }
        return browser;
    }

    pageOf(id: number, tab: string): Page {
        const page = this.running(id).pages.find((one) => one.id === tab);
        if (!page) {
            throw new Error(`the browser of \`${this.task(id).name}\`: it has no tab ${tab}`);
        }
        return page;
    }

    shownPage(id: number): Page {
        return this.pageOf(id, this.running(id).shown ?? "");
    }

    show(id: number, tab: string | null, viewport: Viewport): BrowserState {
        const browser = this.start(id);
        const wanted = tab ?? browser.shown ?? browser.pages[0]?.id ?? null;
        if (wanted !== null && !browser.pages.some((page) => page.id === wanted)) {
            throw new Error(`the browser of \`${this.task(id).name}\`: it has no such tab`);
        }
        browser.shown = wanted;
        browser.viewport = viewport;
        this.told(id);
        this.draw(id);
        return this.state(id);
    }

    hide(id: number) {
        const browser = this.browsers.get(id);
        if (browser) {
            browser.shown = null;
        }
    }

    go(id: number, tab: string, go: BrowserGo) {
        const page = this.pageOf(id, tab);
        if (typeof go === "object") {
            this.goTo(id, page, mockAddress(go.address));
        } else if (go === "back") {
            page.current = Math.max(0, page.current - 1);
        } else if (go === "forward") {
            page.current = Math.min(page.history.length - 1, page.current + 1);
        }
        this.arrived(id, page);
    }

    goTo(id: number, page: Page, url: string) {
        if (url.includes("refused")) {
            throw new Error(
                `the browser of \`${this.task(id).name}\`: net::ERR_CONNECTION_REFUSED`,
            );
        }
        page.history = [...page.history.slice(0, page.current + 1), url];
        page.current = page.history.length - 1;
        page.typed = "";
        page.click = null;
    }

    tabNew(id: number, url: string | null): string {
        const browser = this.start(id);
        const page = this.page(url ? mockAddress(url) : "about:blank");
        browser.pages.push(page);
        this.told(id);
        return page.id;
    }

    tabClose(id: number, tab: string) {
        const browser = this.running(id);
        browser.pages = browser.pages.filter((page) => page.id !== tab);
        if (browser.shown === tab) {
            browser.shown = null;
        }
        this.told(id);
    }

    /**
     * What `show_browser` does for session `session`: the task's browser on `url`, in a tab of
     * the session's own, and the pane asked to show it. Returns the tab.
     */
    shownFor(id: number, session: number, url: string): string {
        const browser = this.start(id);
        const address = mockAddress(url);
        const own = browser.pages.findLast((page) => page.actedBy === session);
        const free = browser.pages.find(
            (page) => page.actedBy === null && page.history[page.current] === "about:blank",
        );
        const page = own ?? free ?? this.page(address);
        if (!browser.pages.includes(page)) {
            browser.pages.push(page);
        }
        if (page.history[page.current] !== address) {
            this.goTo(id, page, address);
        }
        this.actedIn(id, session, page);
        this.emit({ kind: "show", task_id: id, session_id: session, tab: page.id });
        return page.id;
    }

    /** What a session's browser tools do in its tab: a click where it points, or text typed. */
    actedFor(
        id: number,
        session: number,
        act: { click?: { x: number; y: number }; type?: string },
    ) {
        const page = this.running(id).pages.findLast((one) => one.actedBy === session);
        if (!page) {
            return;
        }
        page.click = act.click ?? page.click;
        page.typed += act.type ?? "";
        this.actedIn(id, session, page);
    }

    /** The session acted in `page`: the tab says so, and a pane on screen goes to it. */
    private actedIn(id: number, session: number, page: Page) {
        const browser = this.running(id);
        page.actedBy = session;
        if (browser.shown !== null) {
            browser.shown = page.id;
        }
        this.arrived(id, page);
    }

    console(id: number, tab: string): ConsoleLine[] {
        this.sampleRunning();
        const page = this.browsers.get(id)?.pages.find((one) => one.id === tab);
        return page ? [...page.console] : [];
    }

    /** One sample task's browser runs already, with a few lines in its console. */
    private sampleRunning() {
        const tasks = this.host.projects().flatMap((project) => project.tasks);
        const task = tasks.find((one) => one.browser);
        if (!task || this.browsers.has(task.id)) {
            return;
        }
        const page = this.page(task.url ?? "about:blank");
        this.log(page, "info", "[vite] connected.");
        this.log(page, "warning", 'Image with src "/hero.jpg" has no alt text.');
        this.log(page, "error", "TypeError: Cannot read properties of undefined (reading 'price')");
        this.browsers.set(task.id, { pages: [page], shown: null, viewport: null });
    }
}

/** What a sample browser's tab says of itself. */
function tabOf(page: Page): BrowserTab {
    const url = page.history[page.current] ?? "about:blank";
    return {
        id: page.id,
        title: titleOf(url),
        url,
        loading: false,
        can_go_back: page.current > 0,
        can_go_forward: page.current < page.history.length - 1,
        dialog: page.dialog,
        acted_by: page.actedBy,
    };
}

/** What a key on the picture does to the sample page's one field. */
function keyed(page: Page, input: KeyInput) {
    if (input.kind !== "down" || input.modifiers.meta || input.modifiers.ctrl) {
        return;
    }
    if (input.key === "Backspace") {
        page.typed = page.typed.slice(0, -1);
    } else if ([...input.key].length === 1) {
        page.typed += input.key;
    }
}

const MISSING =
    "no Google Chrome, Chromium, Microsoft Edge or Brave was found: install one, or set the browser's program in Settings";

export function browserApi(browsers: MockBrowsers, host: BrowserHost): BrowserApi {
    return {
        browserSetup: async () => {
            const missing = browsers.missing();
            return {
                name: missing ? null : "Google Chrome",
                path: missing
                    ? null
                    : "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
                install_command: "brew install --cask google-chrome",
                chosen: host.settings().browser_program !== "",
            };
        },
        browserInstall: async () => {
            throw new Error("could not set up a browser: the sample data installs nothing");
        },
        browserState: async (id) => {
            browsers.task(id);
            return browsers.state(id);
        },
        browserOpen: async (id) => {
            browsers.start(id);
            return browsers.state(id);
        },
        browserStop: async (id) => browsers.stop(id),
        browserShow: async (id, tab, viewport) => browsers.show(id, tab, viewport),
        browserHide: async (id) => browsers.hide(id),
        browserMouse: async (id, input) => {
            if (input.kind === "down") {
                browsers.shownPage(id).click = { x: input.x, y: input.y };
                browsers.draw(id);
            }
        },
        browserKey: async (id, input) => {
            keyed(browsers.shownPage(id), input);
            browsers.draw(id);
        },
        browserText: async (id, text) => {
            browsers.shownPage(id).typed += text;
            browsers.draw(id);
        },
        browserGo: async (id, tab, go) => browsers.go(id, tab, go),
        browserTabNew: async (id, url) => browsers.tabNew(id, url),
        browserTabClose: async (id, tab) => browsers.tabClose(id, tab),
        browserDialog: async (id, tab) => {
            browsers.pageOf(id, tab).dialog = null;
            browsers.told(id);
        },
        browserConsole: async (id, tab) => browsers.console(id, tab),
        onBrowserEvent: async (listener) => browsers.listen(listener),
    };
}

/**
 * A picture of the made-up page, as a JPEG in base64, drawn on a canvas where there is one.
 * `null` where there is none, as in the tests: the sample screenshot stands in.
 */
function drawn(page: Page, viewport: Viewport): string | null {
    const canvas = typeof document === "undefined" ? null : document.createElement("canvas");
    const context = canvas?.getContext("2d");
    if (!(canvas && context)) {
        return null;
    }
    const scale = Math.max(1, viewport.scale);
    canvas.width = viewport.width * scale;
    canvas.height = viewport.height * scale;
    context.scale(scale, scale);
    const url = page.history[page.current] ?? "about:blank";
    context.fillStyle = "#ffffff";
    context.fillRect(0, 0, viewport.width, viewport.height);
    if (url !== "about:blank") {
        sitePainted(context, url, page, viewport.width);
    }
    if (page.click) {
        context.fillStyle = "rgba(220, 38, 38, 0.6)";
        context.beginPath();
        context.arc(page.click.x, page.click.y, 8, 0, Math.PI * 2);
        context.fill();
    }
    return canvas.toDataURL("image/jpeg", 0.8).split(",")[1] ?? null;
}

/** The sample site: a bar, a heading, a field that shows what was typed, and a few products. */
function sitePainted(context: CanvasRenderingContext2D, url: string, page: Page, width: number) {
    context.fillStyle = "#1f2937";
    context.fillRect(0, 0, width, 48);
    context.fillStyle = "#ffffff";
    context.font = "bold 18px sans-serif";
    context.fillText(titleOf(url), 16, 30);
    context.fillStyle = "#111827";
    context.font = "15px sans-serif";
    context.fillText(url, 16, 80);
    context.strokeStyle = "#9ca3af";
    context.strokeRect(16, 100, Math.min(360, width - 32), 34);
    context.fillText(page.typed || "Search products…", 26, 122);
    ["Linen shirt: 49 €", "Canvas tote: 25 €", "Wool beanie: 19 €"].forEach((item, at) => {
        context.fillStyle = "#f3f4f6";
        context.fillRect(16, 156 + at * 64, Math.min(360, width - 32), 52);
        context.fillStyle = "#111827";
        context.fillText(item, 28, 187 + at * 64);
    });
}

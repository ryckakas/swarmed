// The tasks' browsers in the sample data, as the core runs them (`browser.rs`): a Chrome per
// task, started when the pane opens. Its pictures are of a made-up page drawn here, which
// follows the address, the clicks and what is typed, so that the pane can be tried in a plain
// browser. An address with `console` in it logs a line, one with `alert` opens a dialog, one
// with `refused` fails as Chrome does. A browser program in the settings with `missing` in it
// is not found, so that the offer to install one shows. The mock agent acts in it as an
// agent's browser tools would (`browse`): in a tab of its session's own, which the pane follows.
// Its parts can be picked as Chrome's inspect mode picks an element (`browserPick`): the one
// under the pointer is outlined, and a click picks it, with a picture cut from the page.

import type { SampleElement } from "../elements";
import { imageFolder } from "../images";
import type { Api } from "./client";
import { MOCK_ATTACHMENTS } from "./mockImages";
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
    PickedElement,
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
    /** Keeps a picture for a session, as a `data:` address, where its images are read from. */
    keepImage(path: string, source: string): void;
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
    /** While an element is picked, the part of the page the pointer is over, outlined. */
    hover: SamplePart | null;
}

interface Browser {
    pages: Page[];
    shown: string | null;
    /** The tab shown when the pane was last hidden. */
    left: string | null;
    viewport: Viewport | null;
    /** A pick under way in the tab shown, for a session, and what it resolves. */
    picking: { session: number; done(element: PickedElement | null): void } | null;
}

/** A part of the sample site that can be picked, where it is drawn. */
interface SamplePart {
    tag: string;
    selector: string;
    role: string;
    name: string;
    html: string;
    box: { x: number; y: number; width: number; height: number };
}

const PRODUCTS = ["Linen shirt: 49 €", "Canvas tote: 25 €", "Wool beanie: 19 €"];

/** The parts of the sample site at `width`, the outermost first, as `sitePainted` draws them. */
function samplePartsOf(url: string, width: number): SamplePart[] {
    if (url === "about:blank") {
        return [];
    }
    const across = Math.min(360, width - 32);
    const header: SamplePart = {
        tag: "header",
        selector: "body > header",
        role: "banner",
        name: "",
        html: `<header class="bar"><h1>${titleOf(url)}</h1></header>`,
        box: { x: 0, y: 0, width, height: 48 },
    };
    const search: SamplePart = {
        tag: "input",
        selector: "#search",
        role: "searchbox",
        name: "Search products",
        html: `<input id="search" type="search" placeholder="Search products…">`,
        box: { x: 16, y: 100, width: across, height: 34 },
    };
    const products = PRODUCTS.map((item, at) => ({
        tag: "li",
        selector: `ul.products > li:nth-of-type(${at + 1})`,
        role: "listitem",
        name: item,
        html: `<li class="product">${item}</li>`,
        box: { x: 16, y: 156 + at * 64, width: across, height: 52 },
    }));
    return [header, search, ...products];
}

/** The innermost part of the sample site at a point, if any. */
function partAt(url: string, width: number, x: number, y: number): SamplePart | null {
    const inside = ({ box }: SamplePart) =>
        x >= box.x && x <= box.x + box.width && y >= box.y && y <= box.y + box.height;
    return samplePartsOf(url, width).findLast(inside) ?? null;
}

/** Where on its site an address is, as a pick's label says it: `/settings`. */
function placeOf(url: string): string {
    const path = url.replace(/^\w+:\/\/[^/]+/, "").split(/[?#]/)[0];
    return url.startsWith("http") ? path || "/" : url;
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
    | "browserPick"
    | "browserPickEnd"
    | "sessionElements"
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
            hover: null,
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
        const browser: Browser = { pages: [first], shown: null, left: null, viewport: null, picking: null };
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
        const left = browser.pages.find((page) => page.id === browser.left)?.id;
        const wanted = tab ?? browser.shown ?? left ?? browser.pages[0]?.id ?? null;
        if (wanted !== null && !browser.pages.some((page) => page.id === wanted)) {
            throw new Error(`the browser of \`${this.task(id).name}\`: it has no such tab`);
        }
        if (wanted !== browser.shown) {
            this.pickEnd(id);
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
            this.pickEnd(id);
            browser.left = browser.shown ?? browser.left;
            browser.shown = null;
        }
    }

    /** The elements picked for each session, as the core keeps them. */
    private picked = new Map<number, PickedElement[]>();

    /** Starts picking in the tab shown for `session`; resolves once a part is clicked. */
    pick(id: number, session: number): Promise<PickedElement | null> {
        const browser = this.running(id);
        this.shownPage(id);
        this.pickEnd(id);
        return new Promise((done) => {
            browser.picking = { session, done };
        });
    }

    /** Ends the pick under way, without one. */
    pickEnd(id: number) {
        const browser = this.browsers.get(id);
        const picking = browser?.picking;
        if (!(browser && picking)) {
            return;
        }
        browser.picking = null;
        for (const page of browser.pages) {
            page.hover = null;
        }
        picking.done(null);
        this.draw(id);
    }

    /**
     * What the mouse does on the page while a part is picked: the part under it outlined, and
     * picked on a press. `false` while nothing is picked, and the mouse acts on the page.
     */
    picks(id: number, input: { kind: string; x: number; y: number }): boolean {
        const browser = this.browsers.get(id);
        const picking = browser?.picking;
        if (!(browser?.viewport && picking)) {
            return false;
        }
        const page = this.shownPage(id);
        const url = page.history[page.current] ?? "about:blank";
        const part = partAt(url, browser.viewport.width, input.x, input.y);
        page.hover = part;
        if (input.kind === "down" && part) {
            browser.picking = null;
            page.hover = null;
            picking.done(this.keepPick(picking.session, page, part, browser.viewport));
        }
        this.draw(id);
        return true;
    }

    /** Keeps `part` of `page` as a pick of `session`'s, with a picture cut from the page. */
    private keepPick(
        session: number,
        page: Page,
        part: SamplePart,
        viewport: Viewport,
    ): PickedElement {
        const kept = this.picked.get(session) ?? [];
        const number = kept.length + 1;
        const url = page.history[page.current] ?? "about:blank";
        const path = `${imageFolder(MOCK_ATTACHMENTS, session)}/element-${number}.png`;
        const picture = cutOut(page, viewport, part.box);
        if (picture) {
            this.host.keepImage(path, picture);
        }
        const named = part.name ? ` "${part.name}"` : "";
        const element: PickedElement = {
            number,
            label: `${part.tag}${named} on ${placeOf(url)}`,
            url,
            tag: part.tag,
            element_id: part.selector.startsWith("#") ? part.selector.slice(1) : null,
            classes: [],
            selector: part.selector,
            role: part.role,
            name: part.name || null,
            text: part.name,
            html: part.html,
            bounds: part.box,
            styles: [
                { property: "color", value: "rgb(17, 24, 39)" },
                { property: "font-size", value: "15px" },
                { property: "margin", value: "0px" },
            ],
            picture: picture ? path : null,
        };
        this.picked.set(session, [...kept, element]);
        return element;
    }

    /** The elements `numbers` picked for `session`, each once; one never picked is refused. */
    elementsOf(session: number, numbers: number[]): PickedElement[] {
        const kept = this.picked.get(session) ?? [];
        return [...new Set(numbers)].map((number) => {
            const found = kept.find((element) => element.number === number);
            if (!found) {
                throw new Error(
                    `element ${number} was not picked for this session: pick it again in the browser`,
                );
            }
            return found;
        });
    }

    /** The same, as the sample lists them under a message. */
    listed(session: number, numbers: number[] = []): SampleElement[] {
        return this.elementsOf(session, numbers).map((element) => ({
            number: element.number,
            label: element.label,
            url: element.url,
            selector: element.selector,
            role: `${element.role ?? "none"}${element.name ? ` "${element.name}"` : ""}`,
            text: element.text,
            html: element.html,
            picture: element.picture ?? "",
        }));
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
        this.browsers.set(task.id, { pages: [page], shown: null, left: null, viewport: null, picking: null });
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
            if (browsers.picks(id, input)) {
                return;
            }
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
        browserPick: async (id, session) => browsers.pick(id, session),
        browserPickEnd: async (id) => browsers.pickEnd(id),
        sessionElements: async (session, numbers) => browsers.elementsOf(session, numbers),
        onBrowserEvent: async (listener) => browsers.listen(listener),
    };
}

/**
 * A picture of the made-up page, as a JPEG in base64, drawn on a canvas where there is one.
 * `null` where there is none, as in the tests: the sample screenshot stands in.
 */
function drawn(page: Page, viewport: Viewport): string | null {
    const canvas = painted(page, viewport);
    return canvas?.toDataURL("image/jpeg", 0.8).split(",")[1] ?? null;
}

/** The made-up page on a canvas of its own, at the screen's density; `null` without one. */
function painted(page: Page, viewport: Viewport): HTMLCanvasElement | null {
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
    if (page.hover) {
        outlined(context, page.hover);
    }
    if (page.click) {
        context.fillStyle = "rgba(220, 38, 38, 0.6)";
        context.beginPath();
        context.arc(page.click.x, page.click.y, 8, 0, Math.PI * 2);
        context.fill();
    }
    return canvas;
}

/** A part outlined as Chrome's inspect mode outlines the element under the pointer. */
function outlined(context: CanvasRenderingContext2D, part: SamplePart) {
    const { x, y, width, height } = part.box;
    context.fillStyle = "rgba(111, 168, 220, 0.5)";
    context.fillRect(x, y, width, height);
    context.font = "12px sans-serif";
    const said = `${part.tag}  ${Math.round(width)} × ${Math.round(height)}`;
    const top = y + height + 6;
    context.fillStyle = "#ffffff";
    context.fillRect(x, top, context.measureText(said).width + 12, 22);
    context.fillStyle = "#881280";
    context.fillText(said, x + 6, top + 15);
}

/**
 * A picture of `box` of the page and a margin around it, as a PNG `data:` address, as the core
 * cuts one from Chrome's; `null` where there is no canvas, as in the tests.
 */
function cutOut(page: Page, viewport: Viewport, box: SamplePart["box"]): string | null {
    const whole = painted({ ...page, hover: null, click: null }, viewport);
    const picture = whole && document.createElement("canvas");
    const context = picture?.getContext("2d");
    if (!(whole && picture && context)) {
        return null;
    }
    const margin = 12;
    const scale = Math.max(1, viewport.scale);
    const left = Math.max(0, box.x - margin);
    const top = Math.max(0, box.y - margin);
    const width = Math.min(viewport.width, box.x + box.width + margin) - left;
    const height = Math.min(viewport.height, box.y + box.height + margin) - top;
    picture.width = width * scale;
    picture.height = height * scale;
    const { width: across, height: down } = picture;
    context.drawImage(whole, left * scale, top * scale, across, down, 0, 0, across, down);
    return picture.toDataURL("image/png");
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

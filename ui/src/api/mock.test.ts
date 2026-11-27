import { afterEach, describe, expect, it, vi } from "vitest";
import { createMockApi, suggestedTitle } from "./mock";
import type { BrowserEvent, SessionEvent } from "./types";

// The sample `Api` the UI runs on in a plain browser. These tests are about the mock alone:
// what the store makes of an `Api` is tested with the store.

const sessionOf = async (api: ReturnType<typeof createMockApi>, id: number) =>
    (await api.overview())
        .flatMap((project) => project.tasks)
        .flatMap((task) => task.sessions)
        .find((session) => session.id === id);

describe("the mock's notes", () => {
    it("are kept per repository and per task, and marked in the tree", async () => {
        const api = createMockApi();
        await api.setNote("bonsai-lint", "rust-support", "token: abc");
        expect(await api.note("bonsai-lint", "rust-support")).toBe("token: abc");
        expect(await api.note("bonsai-lint", null)).toBe("");
        const marked = (await api.overview()).find((project) => project.name === "bonsai-lint");
        expect(marked?.has_note).toBe(false);
        expect(marked?.tasks[0]?.has_note).toBe(true);

        await api.setNote("bonsai-lint", "rust-support", "  ");
        const cleared = (await api.overview()).find((project) => project.name === "bonsai-lint");
        expect(cleared?.tasks[0]?.has_note).toBe(false);
    });
});

describe("the mock's issues", () => {
    it("lists a repository's issues by the filters, and says why where there are none", async () => {
        const api = createMockApi();
        const filter = { who: "anyone", state: "open", search: "", limit: 50 } as const;
        const open = await api.projectIssues("storefront", filter);
        expect(open.unavailable).toBeNull();
        expect(open.issues.map((issue) => issue.number)).toContain(142);
        const mine = await api.projectIssues("storefront", { ...filter, who: "assigned" });
        expect(mine.issues.map((issue) => issue.number)).toEqual([428, 142]);
        const closed = await api.projectIssues("storefront", { ...filter, state: "closed" });
        expect(closed.issues.every((issue) => issue.state === "closed")).toBe(true);
        const off = await api.projectIssues("fsum", filter);
        expect(off.issues).toEqual([]);
        expect(off.unavailable).toContain("GitHub host");
    });

    it("reads an issue whole, and a task's issue", async () => {
        const api = createMockApi();
        const detail = await api.projectIssue("storefront", 428);
        expect(detail.body).toContain("<script>");
        expect(detail.comments.some((comment) => comment.hidden === "spam")).toBe(true);
        await expect(api.projectIssue("storefront", 1)).rejects.toThrow("Could not resolve");
        expect((await api.taskIssue("storefront", "shop-142"))?.number).toBe(142);
        expect(await api.taskIssue("storefront", "invoice-export")).toBeNull();
    });
});

describe("the mock's removing", () => {
    it("refuses a task with unsaved work until told to remove it anyway", async () => {
        const api = createMockApi();
        await expect(api.taskRemove("storefront", "invoice-export", false)).rejects.toThrow(
            "uncommitted changes",
        );
        await api.taskRemove("storefront", "invoice-export", true);
        const storefront = (await api.overview()).find((project) => project.name === "storefront");
        expect(storefront?.tasks.map((task) => task.name)).not.toContain("invoice-export");
    });
});

describe("the mock's queue of waiting messages", () => {
    it("can be rewritten and emptied while it waits", async () => {
        const api = createMockApi();
        const [first] = await api.sessionEnqueue(3, "1st");
        await api.sessionEnqueue(3, "second");
        const edited = await api.sessionQueueEdit(3, first?.id ?? 0, "first");
        expect(edited.map((message) => message.text)).toEqual(["first", "second"]);
        const left = await api.sessionQueueEdit(3, first?.id ?? 0, " ");
        expect(left.map((message) => message.text)).toEqual(["second"]);
        await api.sessionSendQueued(3);
        expect(await api.sessionQueue(3)).toEqual([]);
    });

    it("sends the next one when a turn is stopped", async () => {
        const api = createMockApi();
        await api.sessionSend(3, "slow");
        await api.sessionEnqueue(3, "after");
        await api.sessionInterrupt(3);
        expect(await api.sessionQueue(3)).toEqual([]);
        const said = await api.sessionEvents(3);
        expect(said.at(-1)).toEqual({ type: "user_message", text: "after" });
    });

    it("sends one now ahead of the others", async () => {
        const api = createMockApi();
        await api.sessionEnqueue(3, "first");
        const queue = await api.sessionEnqueue(3, "second");
        await api.sessionSendNow(3, queue[1]?.id ?? 0);
        const left = await api.sessionQueue(3);
        expect(left.map((message) => message.text)).toEqual(["first"]);
        const said = await api.sessionEvents(3);
        expect(said.at(-1)).toEqual({ type: "user_message", text: "second" });
    });

    it("puts one first, never sending it into a turn it cannot stop", async () => {
        const api = createMockApi();
        await api.sessionSend(3, "hello");
        await api.sessionEnqueue(3, "first");
        const queue = await api.sessionEnqueue(3, "second");
        await api.sessionSendNow(3, queue[1]?.id ?? 0);
        const left = await api.sessionQueue(3);
        expect(left.map((message) => message.text)).toEqual(["second", "first"]);
        const said = await api.sessionEvents(3);
        expect(said.at(-1)).toEqual({ type: "user_message", text: "hello" });
    });
});

describe("the mock's mentions", () => {
    it("lists what a session can read, and sends a mention checked and listed", async () => {
        const api = createMockApi();
        const projects = await api.overview();
        const homeOf = (id: number) =>
            projects.find((project) =>
                project.tasks.some((task) => task.sessions.some((session) => session.id === id)),
            );
        const listed = await api.sessionReachable(3);
        expect(listed.map((session) => session.id)).not.toContain(3);
        expect(listed.length).toBeGreaterThan(0);
        // A repository's session reads only its own repository's.
        const home = homeOf(3)?.name;
        expect(listed.every((session) => session.project === home)).toBe(true);
        const other = listed[0]?.id ?? 0;
        await api.sessionSend(3, `see [@${other} it]`, [], [other]);
        const said = (await api.sessionEvents(3)).find(
            (event) => event.type === "user_message" && event.text.startsWith("see "),
        );
        expect(said).toMatchObject({ text: expect.stringContaining(`- session ${other} (`) });

        // One of another repository is refused before anything goes.
        const elsewhere = projects
            .filter((project) => project.name !== home && !project.chats)
            .flatMap((project) => project.tasks.flatMap((task) => task.sessions))[0];
        const away = elsewhere?.id ?? 0;
        await expect(api.sessionSend(3, "look", [], [away])).rejects.toThrow(
            `session ${away} cannot be mentioned here`,
        );
    });
});

describe("the mock's pull requests and issues", () => {
    it("offers a repository's to # and sends one listed under the message", async () => {
        const api = createMockApi();
        const projects = await api.overview();
        const storefront = projects.find((project) => project.name === "storefront");
        const session = storefront?.tasks.flatMap((task) => task.sessions)[0];
        const id = session?.id ?? 0;
        const listed = await api.projectGithubItems("storefront", "");
        expect(listed.unavailable).toBeNull();
        const kinds = new Set(listed.items.map((item) => item.kind));
        expect(kinds).toEqual(new Set(["pull", "issue"]));
        // GitHub counts both alike: no number twice.
        const numbers = listed.items.map((item) => item.number);
        expect(new Set(numbers).size).toBe(numbers.length);
        const one = await api.projectGithubItems("storefront", "412");
        expect(one.items.map((item) => item.title)).toEqual(["Fix the redirect loop after login"]);

        await api.projectGithubRead("storefront", [412]);
        await api.sessionSend(id, "see [#412 Fix]", [], [], [412]);
        const said = (await api.sessionEvents(id)).find(
            (event) => event.type === "user_message" && event.text.startsWith("see [#412"),
        );
        expect(said).toMatchObject({
            text: expect.stringContaining(
                '- pull request #412 ("Fix the redirect loop after login", open',
            ),
        });

        // A repository not on GitHub offers none and says why; a number that is not there is refused.
        const fsum = await api.projectGithubItems("fsum", "");
        expect(fsum.items).toEqual([]);
        expect(fsum.unavailable).toContain("known GitHub host");
        await expect(api.projectGithubRead("storefront", [9999])).rejects.toThrow(
            "#9999 cannot be mentioned",
        );
    });
});

describe("the mock's agent setup", () => {
    it("installs an agent, then lets its models be chosen", async () => {
        const api = createMockApi();
        const setups = await api.agentSetups();
        expect(setups.map((setup) => setup.id)).toEqual(["claude", "codex", "opencode"]);
        expect(setups[2]?.installed).toBeNull();
        const installed = await api.agentInstall("opencode");
        expect(installed.configured).toBe(true);
        const chosen = await api.agentConfigure("opencode", ["openrouter/x-ai/grok-4.7"]);
        expect(chosen.models).toEqual(["openrouter/x-ai/grok-4.7"]);
        // Asked again without naming models, the choice stands.
        expect((await api.agentConfigure("opencode", null)).models).toEqual(chosen.models);
    });
});

describe("the mock's MCP servers", () => {
    it("set up here come first, are named by their secrets only, and can be removed", async () => {
        const api = createMockApi();
        const before = await api.mcpServers();
        expect(before.every((server) => server.origin !== "swarmed")).toBe(true);
        const listed = await api.mcpSave({
            name: "tracker",
            command: "npx -y tracker",
            url: "",
            secrets: [["TRACKER_TOKEN", "s3cret"]],
            enabled: true,
        });
        expect(listed[0]).toMatchObject({ name: "tracker", origin: "swarmed", kind: "stdio" });
        expect(JSON.stringify(listed)).not.toContain("s3cret");
        expect(await api.mcpRemove("tracker")).toEqual(before);
    });
});

describe("the mock's built-in repolore", () => {
    afterEach(() => {
        vi.useRealTimers();
    });
    /** What a step returns once its waits have passed, without waiting for them. */
    const done = async <T>(step: Promise<T>): Promise<T> => {
        await vi.advanceTimersByTimeAsync(10_000);
        return step;
    };

    it("walks from not installed through waiting to on, and back off with its copy kept", async () => {
        vi.useFakeTimers();
        const api = createMockApi();
        const printed: string[] = [];
        await api.onHookOutput((output) => {
            if (output.task === "builtin:repolore") {
                printed.push(output.line);
            }
        });
        const [first] = await done(api.builtinMcpServers());
        expect(first).toMatchObject({ name: "repolore", state: "missing", version: null });

        const off = await done(api.builtinMcpInstall("repolore"));
        expect(printed.at(-1)).toContain("repolore-mcp");
        expect(off).toMatchObject({ state: "off", home_ready: false });
        expect(off.existing?.folders.length).toBeGreaterThan(0);

        // The first copy is refused while a refresh runs; nothing is turned on.
        const refused = expect(api.builtinMcpTurnOn("repolore", true)).rejects.toThrow(
            "a refresh is running; try again when it is done",
        );
        await vi.advanceTimersByTimeAsync(10_000);
        await refused;
        const waiting = await done(api.builtinMcpTurnOn("repolore", true));
        expect(waiting.state).toBe("waiting");
        expect(waiting.registrations.map((each) => [each.client, each.runs_here])).toEqual([
            ["Claude Code", true],
            ["Codex", true],
            ["OpenCode", false],
        ]);
        expect(waiting).toMatchObject({ old_server: { relays: 1 }, hook: true });
        expect(waiting.sessions).toEqual([expect.objectContaining({ idle: true })]);

        // A step for the user is never run here.
        await expect(
            api.builtinMcpRemoveRegistration("repolore", "opencode:user"),
        ).rejects.toThrow();
        await done(api.builtinMcpRemoveRegistration("repolore", "claude:user"));
        expect(printed.at(-1)).toContain("~/.claude.json");
        const left = await done(api.builtinMcpRemoveRegistration("repolore", "codex:user"));
        expect(left.registrations.map((each) => each.key)).toEqual(["opencode:user"]);

        // The old server runs while the idle session holds it.
        const held = await done(api.builtinMcpCheck("repolore", false));
        expect(held).toMatchObject({ state: "waiting", registrations: [] });
        expect(held.old_server).not.toBeNull();
        const letGo = await done(api.builtinMcpRestartSessions("repolore"));
        expect(letGo.sessions).toEqual([]);
        const on = await done(api.builtinMcpCheck("repolore", false));
        expect(on).toMatchObject({ state: "on", old_server: null, checked: "offline" });
        expect(on.checks.filter((check) => check.warning)).toHaveLength(1);
        expect(on.checks.every((check) => check.ok)).toBe(true);

        const full = await done(api.builtinMcpCheck("repolore", true));
        expect(full.checked).toBe("full");
        expect(full.checks.length).toBeGreaterThan(on.checks.length);
        await done(api.builtinMcpOpen("repolore"));
        expect((await done(api.builtinMcpServers()))[0]?.page_open).toBe(true);
        expect((await done(api.builtinMcpTrashOld("repolore"))).existing).toBeNull();

        const offAgain = await done(api.builtinMcpTurnOff("repolore"));
        expect(offAgain).toMatchObject({ state: "off", home_ready: true, page_open: false });
    });
});

describe("the mock's tree", () => {
    it("says what the events said: a working session is still working when read again", async () => {
        const api = createMockApi();
        expect((await sessionOf(api, 3))?.status).toBe("idle");
        await api.sessionSend(3, "slow");
        expect((await sessionOf(api, 3))?.status).toBe("working");
        await api.sessionInterrupt(3);
        expect(await sessionOf(api, 3)).toMatchObject({ status: "idle", unread: true });
    });

    it("keeps a new session's title, from its first message and from a renaming", async () => {
        const api = createMockApi();
        const created = await api.sessionNew("storefront", "invoice-export", {
            agent: null,
            model: null,
            supervision: "auto",
            effort: null,
        });
        await api.sessionSend(created.id, "slow");
        expect((await sessionOf(api, created.id))?.title).toBe("slow");
        await api.sessionRename(created.id, "Slow work");
        expect((await sessionOf(api, created.id))?.title).toBe("Slow work");
    });

    it("lists pinned tasks first, and moves each among those pinned as it is", async () => {
        const api = createMockApi();
        // As the sidebar lists them with the others: the task put away is listed apart.
        const tasks = async () =>
            (await api.overview())
                .find((project) => project.name === "storefront")
                ?.tasks.filter((task) => task.done_at === null)
                .map((task) => `${task.name}${task.pinned ? " (pinned)" : ""}`);
        expect(await tasks()).toEqual([
            "fix-login-redirect (pinned)",
            "invoice-export",
            "shop-142",
        ]);
        await api.taskPin("storefront", "shop-142", true);
        await api.taskMove("storefront", "invoice-export", -5);
        expect(await tasks()).toEqual([
            "fix-login-redirect (pinned)",
            "shop-142 (pinned)",
            "invoice-export",
        ]);
        await api.taskPin("storefront", "fix-login-redirect", false);
        expect(await tasks()).toEqual([
            "shop-142 (pinned)",
            "fix-login-redirect",
            "invoice-export",
        ]);
        await api.sessionPin(2, true);
        expect((await sessionOf(api, 2))?.pinned).toBe(true);
    });
});

describe("the mock's agent", () => {
    it("answers `show` with an HTML view, then ends its turn", async () => {
        const api = createMockApi();
        const heard: SessionEvent["event"][] = [];
        await api.onSessionEvent((published) => heard.push(published.event));
        await api.sessionSend(3, "show the invoice totals");
        await expect
            .poll(() => heard.some((event) => event.type === "turn_done"), { timeout: 3000 })
            .toBe(true);
        const reply = heard.find((event) => event.type === "text");
        expect(reply?.type === "text" && reply.text).toContain("```html-view\n<style>");
    });

    it("answers `screenshot` with a browser check whose last call returns an image", async () => {
        const api = createMockApi();
        const heard: SessionEvent["event"][] = [];
        await api.onSessionEvent((published) => heard.push(published.event));
        await api.sessionSend(3, "screenshot");
        await expect
            .poll(() => heard.some((event) => event.type === "turn_done"), { timeout: 5000 })
            .toBe(true);
        const images = heard.flatMap((event) =>
            event.type === "tool_result" ? (event.images ?? []) : [],
        );
        expect(images).toEqual([`${await api.attachmentsRoot()}/3/tool-shot-100-3-1.png`]);
        const [path = ""] = images;
        expect(await api.sessionImage(3, path)).toMatch(/^data:image\/png;base64,iVBORw0KGgo/);
        await expect(api.sessionImage(4, path)).rejects.toThrow("not an image kept");
    });

    it("answers `app` with a call whose server's app shows under it and calls its tools", async () => {
        const api = createMockApi();
        const heard: SessionEvent["event"][] = [];
        await api.onSessionEvent((published) => heard.push(published.event));
        await api.sessionSend(3, "app");
        await expect
            .poll(() => heard.some((event) => event.type === "turn_done"), { timeout: 5000 })
            .toBe(true);
        const app = heard.find((event) => event.type === "app");
        if (app?.type !== "app" || app.document === null) {
            throw new Error("no app");
        }
        expect(app.callable).toEqual(["chart", "refresh"]);
        const shown = await api.appUrl(3, app.tool_use_id, app.document, app.csp);
        expect(shown.reaches).toEqual(["https://api.invoices.example.com"]);
        expect(shown.refused).toEqual(["http://plain.example.com (not an https origin)"]);
        const called = await api.sessionAppCall(3, app.tool_use_id, "refresh", { months: 6 });
        expect(called).toMatchObject({ structuredContent: { bars: expect.any(Array) } });
        await expect(api.sessionAppCall(3, app.tool_use_id, "drop", {})).rejects.toThrow(
            "may not call",
        );
    });

    it("is given a better title while it writes its first reply, and only then", async () => {
        const api = createMockApi();
        const heard: SessionEvent["event"][] = [];
        await api.onSessionEvent((published) => heard.push(published.event));
        const created = await api.sessionNew("storefront", "invoice-export", {
            agent: null,
            model: null,
            supervision: "auto",
            effort: null,
        });
        const turn = async (message: string) => {
            heard.length = 0;
            await api.sessionSend(created.id, message);
            await expect.poll(() => heard.some((event) => event.type === "turn_done")).toBe(true);
            return heard.map((event) => event.type);
        };

        const first = await turn("fix the login redirect");
        const renamed = first.indexOf("renamed");
        // Between two pieces of the reply: what the store must not lose the text over.
        expect(first.slice(0, renamed)).toContain("text_delta");
        expect(first.slice(renamed, first.indexOf("text"))).toContain("text_delta");
        expect((await sessionOf(api, created.id))?.title).toBe(
            suggestedTitle("fix the login redirect"),
        );

        expect(await turn("and the tests")).not.toContain("renamed");
    });
});

describe("the mock's terminal", () => {
    it("says where its replay ends and where each piece of output starts", async () => {
        const api = createMockApi();
        const offsets: number[] = [];
        let printed = "";
        await api.onTerminalOutput((output) => {
            offsets.push(output.offset);
            printed += output.data;
        });
        expect(await api.terminalOpen(1, 80, 24)).toEqual({ replay: "", end: 0, run: 0 });
        await expect.poll(() => printed).not.toBe("");
        await api.terminalWrite(1, "ls");
        expect(offsets).toEqual([0, printed.length - 2]);
        const again = await api.terminalOpen(1, 80, 24);
        expect(again).toEqual({ replay: printed, end: printed.length, run: 0 });
    });

    it("shows a Claude Code again on the same command, and another run on another", async () => {
        const api = createMockApi();
        const heard: { run: number; ended: boolean }[] = [];
        await api.onClaudeTerminalOutput(({ run, ended }) => heard.push({ run, ended }));
        const first = await api.claudeTerminalOpen(1, "vim", 80, 24);
        await expect.poll(() => heard).toEqual([{ run: first.run, ended: false }]);
        expect((await api.claudeTerminalOpen(1, "vim", 80, 24)).run).toBe(first.run);
        const other = await api.claudeTerminalOpen(1, "privacy-settings", 80, 24);
        expect(other.run).not.toBe(first.run);
        await api.claudeTerminalWrite(1, "q");
        expect(heard.at(-1)).toEqual({ run: other.run, ended: true });
    });
});

describe("the mock's pasted images", () => {
    const PNG = "iVBORw0KGgo=";

    it("keeps them as the session's own, attaches them by path and shows them again", async () => {
        const api = createMockApi();
        const root = await api.attachmentsRoot();
        const [path] = await api.sessionImagesKeep(3, [PNG]);
        expect(path).toBe(`${root}/3/pasted-1.png`);
        expect(await api.sessionImage(3, path ?? "")).toBe(`data:image/png;base64,${PNG}`);
        await expect(api.sessionImage(4, path ?? "")).rejects.toThrow("not an image kept");
        await expect(api.sessionImagesKeep(3, ["SUkqAA=="])).rejects.toThrow("a TIFF image");
    });
});

describe("the mock's browsers", () => {
    const view = { width: 640, height: 480, scale: 1 };

    it("start with the pane, draw the tab shown, follow where it goes and stop", async () => {
        const api = createMockApi();
        const tasks = (await api.overview()).flatMap((project) => project.tasks);
        const running = tasks.find((task) => task.browser);
        const idle = tasks.find((task) => !(task.browser || task.archived || task.done_at));
        if (!(running && idle)) {
            throw new Error(
                "the sample data has a task whose browser runs, and one whose does not",
            );
        }
        expect((await api.browserState(running.id)).running).toBe(true);
        const heard: BrowserEvent[] = [];
        await api.onBrowserEvent((event) => heard.push(event));

        const shown = await api.browserShow(idle.id, null, view);
        const tab = shown.shown ?? "";
        expect(shown.running).toBe(true);
        expect(heard.some((event) => event.kind === "frame" && event.tab === tab)).toBe(true);
        await api.browserGo(idle.id, tab, { address: "localhost:3000/console" });
        const lines = await api.browserConsole(idle.id, tab);
        expect(lines.map((line) => line.text)).toEqual([
            "hello from http://localhost:3000/console",
        ]);
        await expect(
            api.browserGo(idle.id, tab, { address: "localhost:9/refused" }),
        ).rejects.toThrow("ERR_CONNECTION_REFUSED");
        const marked = (await api.overview()).flatMap((project) => project.tasks);
        expect(marked.find((task) => task.id === idle.id)?.browser).toBe(true);

        // Hidden and shown again, the pane is on the tab it was on, not back on the first.
        const second = await api.browserTabNew(idle.id, null);
        await api.browserShow(idle.id, second, view);
        await api.browserHide(idle.id);
        expect((await api.browserShow(idle.id, null, view)).shown).toBe(second);

        await api.browserStop(idle.id);
        expect((await api.browserState(idle.id)).running).toBe(false);
        await expect(api.browserText(idle.id, "late")).rejects.toThrow("not running");
    });

    it("are acted in by the agent's tools on `browse`, in a tab the pane follows", async () => {
        const api = createMockApi();
        const session = 3;
        const task = (await api.overview())
            .flatMap((project) => project.tasks)
            .find((one) => one.sessions.some((known) => known.id === session));
        if (!task) {
            throw new Error("the sample session has a task");
        }
        const heard: BrowserEvent[] = [];
        await api.onBrowserEvent((event) => heard.push(event));
        const said: SessionEvent["event"][] = [];
        await api.onSessionEvent((published) => said.push(published.event));
        await api.browserShow(task.id, null, view);
        await api.sessionSend(session, "browse");
        await expect
            .poll(() => said.some((event) => event.type === "turn_done"), { timeout: 5000 })
            .toBe(true);
        const asked = heard.find((event) => event.kind === "show");
        expect(asked?.kind === "show" && asked.session_id).toBe(session);
        const state = await api.browserState(task.id);
        const own = state.tabs.filter((tab) => tab.acted_by === session);
        expect(own.map((tab) => tab.url)).toEqual(["http://localhost:5173/products"]);
        expect(state.shown).toBe(own[0]?.id);
        const failed = said.some((event) => event.type === "tool_result" && event.is_error);
        expect(failed).toBe(false);
    });

    it("pick a part of the page with a click and list it under a message to the session", async () => {
        const api = createMockApi();
        const session = 3;
        const task = (await api.overview())
            .flatMap((project) => project.tasks)
            .find((one) => one.sessions.some((known) => known.id === session));
        if (!task) {
            throw new Error("the sample session has a task");
        }
        const state = await api.browserShow(task.id, null, view);
        await api.browserGo(task.id, state.shown ?? "", { address: "localhost:3000/shop" });
        const ended = api.browserPick(task.id, session);
        await api.browserPickEnd(task.id);
        expect(await ended).toBeNull();

        const picking = api.browserPick(task.id, session);
        const move = { kind: "move" as const, button: "none" as const, buttons: 0, click_count: 0 };
        const held = { alt: false, ctrl: false, meta: false, shift: false };
        const at = { x: 40, y: 180, delta_x: 0, delta_y: 0, modifiers: held };
        await api.browserMouse(task.id, { ...at, ...move });
        await api.browserMouse(task.id, {
            ...at,
            kind: "down",
            button: "left",
            buttons: 1,
            click_count: 1,
        });
        const element = await picking;
        expect(element).toMatchObject({
            number: 1,
            tag: "li",
            label: 'li "Linen shirt: 49 €" on /shop',
        });
        expect(await api.sessionElements(session, [1])).toEqual([element]);
        await expect(api.sessionElements(session, [2])).rejects.toThrow("element 2 was not picked");

        const said: SessionEvent["event"][] = [];
        await api.onSessionEvent((published) => said.push(published.event));
        await api.sessionSend(session, "make it blue [Element 1: li]", [], [], [], [1]);
        const sent = said.find((event) => event.type === "user_message");
        expect(sent?.type === "user_message" && sent.text).toContain(
            '- element 1: li "Linen shirt',
        );
    });

    it("offer to install one when the program set is missing", async () => {
        const api = createMockApi();
        const settings = await api.settings();
        await api.setSettings({ ...settings, browser_program: "/missing/chrome" });
        expect((await api.browserSetup()).name).toBeNull();
        const task = (await api.overview())[0]?.tasks[0];
        await expect(api.browserOpen(task?.id ?? 0)).rejects.toThrow("no Google Chrome");
    });
});

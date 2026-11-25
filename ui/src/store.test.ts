import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { newSession } from "./actions";
import { api as moduleApi } from "./api";
import type { Api } from "./api/client";
import { createMockApi } from "./api/mock";
import type { AccountUsage, AgentEvent, SessionEvent } from "./api/types";
import { NO_DRAFT } from "./sending";
import {
    appendEvent,
    configProblemsSaid,
    connect,
    heardWhileReading,
    menuChosen,
    type Reading,
    useStore,
    withEvent,
    withStreamed,
} from "./store";
import { findSession, findSessionHome } from "./tree";

const text = (value: string): AgentEvent => ({ type: "text", text: value });
const published = (sessionId: number, event: AgentEvent): SessionEvent => ({
    session_id: sessionId,
    status: null,
    event,
});

beforeEach(() => {
    useStore.setState({ projects: [], events: {}, selectedSessionId: null, problem: null });
});

describe("appendEvent", () => {
    it("adds to an open transcript and ignores sessions that are not open", () => {
        const events = { 1: [text("a")] };
        expect(appendEvent(events, published(1, text("b")))).toEqual({ 1: [text("a"), text("b")] });
        expect(appendEvent(events, published(2, text("b")))).toBe(events);
    });
});

describe("open", () => {
    it("selects the session and loads its history once", async () => {
        let reads = 0;
        const api: Api = {
            ...createMockApi(),
            sessionEvents: async () => {
                reads += 1;
                return [text("history")];
            },
        };
        await useStore.getState().open(api, 7);
        await useStore.getState().open(api, 7);

        expect(useStore.getState().selectedSessionId).toBe(7);
        expect(useStore.getState().events[7]).toEqual([text("history")]);
        expect(reads).toBe(1);
    });

    it("reads again when an event arrives while the history is loading", async () => {
        const stored = [text("first")];
        let release: () => void = () => undefined;
        const gate = new Promise<void>((resolve) => {
            release = resolve;
        });
        let reads = 0;
        const api: Api = {
            ...createMockApi(),
            sessionEvents: async () => {
                reads += 1;
                const snapshot = [...stored];
                if (reads === 1) {
                    await gate;
                }
                return snapshot;
            },
        };

        const opening = useStore.getState().open(api, 7);
        // The agent says something after the first read started but before it returned.
        stored.push(text("second"));
        useStore.getState().apply(published(7, text("second")));
        release();
        await opening;

        expect(reads).toBe(2);
        expect(useStore.getState().events[7]).toEqual([text("first"), text("second")]);
    });

    it("does not read again for the pieces of a reply, and shows them once it has read", async () => {
        let release: () => void = () => undefined;
        const gate = new Promise<void>((resolve) => {
            release = resolve;
        });
        let reads = 0;
        const api: Api = {
            ...createMockApi(),
            sessionEvents: async () => {
                reads += 1;
                await gate;
                return [text("first")];
            },
        };
        const delta = (value: string): AgentEvent => ({ type: "text_delta", text: value });

        const opening = useStore.getState().open(api, 7);
        // The agent is in the middle of a reply: its pieces are published and never stored.
        useStore.getState().apply(published(7, delta("Rain ")));
        useStore.getState().apply(published(7, delta("falls")));
        useStore.getState().apply(published(7, { type: "config_changed", kind: "rule" }));
        release();
        await opening;

        expect(reads).toBe(1);
        expect(useStore.getState().events[7]).toEqual([text("first"), delta("Rain falls")]);
        // The rest of the reply joins what was heard while reading.
        useStore.getState().apply(published(7, delta(".")));
        expect(useStore.getState().events[7]).toEqual([text("first"), delta("Rain falls.")]);
    });

    it("reports a history that cannot be read", async () => {
        const api: Api = {
            ...createMockApi(),
            sessionEvents: async () => {
                throw new Error("session 7 not found");
            },
        };
        await useStore.getState().open(api, 7);
        expect(useStore.getState().problem).toBe("Error: session 7 not found");
    });
});

describe("withEvent", () => {
    const delta = (value: string): AgentEvent => ({ type: "text_delta", text: value });

    it("grows a reply piece by piece and swaps in the whole reply", () => {
        let events = [text("earlier")];
        events = withEvent(events, delta("Rain "));
        events = withEvent(events, delta("falls."));
        expect(events).toEqual([text("earlier"), delta("Rain falls.")]);

        events = withEvent(events, text("Rain falls."));
        expect(events).toEqual([text("earlier"), text("Rain falls.")]);
    });

    it("drops unfinished pieces when the turn ends without the whole reply", () => {
        const done: AgentEvent = { type: "turn_done", is_error: true, result: null, cost_usd: 0 };
        expect(withEvent([delta("Rai")], done)).toEqual([done]);
        const exited: AgentEvent = { type: "exited", code: 1, stderr: "" };
        expect(withEvent([delta("Rai")], exited)).toEqual([exited]);
    });

    it("keeps the reply so far when something that is not part of it arrives meanwhile", () => {
        const renamed: AgentEvent = { type: "renamed", title: "Rain" };
        let events = withEvent([text("earlier"), delta("Rain ")], renamed);
        expect(events).toEqual([text("earlier"), renamed, delta("Rain ")]);
        // The reply goes on growing, and the whole of it still takes the pieces' place.
        events = withEvent(events, delta("falls."));
        expect(events).toEqual([text("earlier"), renamed, delta("Rain falls.")]);
        events = withEvent(events, text("Rain falls."));
        expect(events).toEqual([text("earlier"), renamed, text("Rain falls.")]);
    });

    it("keeps it for a mark, a sub-agent's word and a usage limit as well", () => {
        const others: AgentEvent[] = [
            { type: "marked", colour: "green", label: "done" },
            { type: "sub", parent: "t1", event: text("Looking.") },
            { type: "rate_limit", status: "allowed_warning", resets_at: null, raw: null },
        ];
        for (const other of others) {
            expect(withEvent([delta("Rain ")], other)).toEqual([other, delta("Rain ")]);
        }
    });
});

describe("heardWhileReading", () => {
    const delta = (value: string): AgentEvent => ({ type: "text_delta", text: value });
    const fresh = (): Reading => ({ missedEvent: false, streamed: "" });

    it("reads again only for events the core stores", () => {
        const unstored: AgentEvent[] = [
            delta("Rain"),
            { type: "queue", messages: [] },
            { type: "config_changed", kind: "rule" },
            { type: "deleted" },
        ];
        for (const event of unstored) {
            const reading = fresh();
            heardWhileReading(reading, event);
            expect(reading.missedEvent).toBe(false);
        }
        const reading = fresh();
        heardWhileReading(reading, { type: "renamed", title: "Rain" });
        expect(reading.missedEvent).toBe(true);
    });

    it("keeps the pieces of a reply until the whole reply is there to be read", () => {
        const reading = fresh();
        heardWhileReading(reading, delta("Rain "));
        heardWhileReading(reading, { type: "renamed", title: "Rain" });
        heardWhileReading(reading, delta("falls"));
        expect(reading.streamed).toBe("Rain falls");
        expect(withStreamed([text("first")], reading.streamed)).toEqual([
            text("first"),
            delta("Rain falls"),
        ]);

        heardWhileReading(reading, text("Rain falls."));
        expect(reading).toEqual({ missedEvent: true, streamed: "" });
        const history = [text("first"), text("Rain falls.")];
        expect(withStreamed(history, reading.streamed)).toBe(history);
    });
});

describe("connect", () => {
    it("keeps delivering events after an earlier connection is cleaned up", async () => {
        const api = createMockApi();
        const disconnectFirst = connect(api);
        connect(api);
        disconnectFirst();
        await useStore.getState().open(api, 1);
        // Let the first connection's cleanup, which waits on a promise, run.
        await new Promise((resolve) => setTimeout(resolve, 0));

        const before = useStore.getState().events[1]?.length ?? 0;
        await api.sessionAllow(1, "sample-request");

        // The answer and the tool's result, each delivered once.
        expect(useStore.getState().events[1]).toHaveLength(before + 2);
    });
});

describe("removing", () => {
    it("takes a repository out of the tree, and the open session with it", async () => {
        const api = createMockApi();
        await useStore.getState().refresh(api);
        await useStore.getState().open(api, 4);
        await api.projectRemove("bonsai-lint", false);
        await useStore.getState().refresh(api);
        const { projects, selectedSessionId } = useStore.getState();
        expect(projects.map((project) => project.name)).not.toContain("bonsai-lint");
        expect(selectedSessionId).toBeNull();
    });

    it("forgets a culled session's conversation, so a later session cannot show it", async () => {
        const api = createMockApi();
        await useStore.getState().refresh(api);
        await useStore.getState().open(api, 4);
        useStore.setState({ queues: { 4: [{ id: 1, text: "next" }] } });
        expect(useStore.getState().events[4]?.length).toBeGreaterThan(0);
        await useStore.getState().deleteSession(api, 4);
        const { events, queues } = useStore.getState();
        expect(events[4]).toBeUndefined();
        expect(queues[4]).toBeUndefined();
    });

    it("forgets what it kept of a removed task's sessions", async () => {
        const api = createMockApi();
        await useStore.getState().refresh(api);
        await useStore.getState().open(api, 4);
        const task = useStore
            .getState()
            .projects.flatMap((project) => project.tasks)
            .find((candidate) => candidate.sessions.some((session) => session.id === 4));
        expect(task).toBeDefined();
        useStore.getState().forgetTasks([task?.id ?? -1]);
        expect(useStore.getState().events[4]).toBeUndefined();
    });
});

describe("the session on screen going", () => {
    const homeOf = (id: number) => {
        const { projects } = useStore.getState();
        const session = findSession(projects, id);
        return session && findSessionHome(projects, session);
    };
    const centre = () => {
        const { selectedSessionId, draftProject } = useStore.getState();
        return { selectedSessionId, draftProject };
    };
    /** Session 4 on screen, after another repository was last chosen for a new task. */
    const onScreen = async (api: Api) => {
        await useStore.getState().refresh(api);
        useStore.setState({ draftProject: "fsum", subView: null });
        await useStore.getState().open(api, 4);
        const home = homeOf(4);
        expect(home?.project.name).not.toBe("fsum");
        return { project: home?.project.name ?? "", task: home?.task.name ?? "" };
    };

    it("opens the new task of its repository when it is closed", async () => {
        const api = createMockApi();
        const { project } = await onScreen(api);
        await useStore.getState().close(api, 4);
        expect(centre()).toEqual({ selectedSessionId: null, draftProject: project });
    });

    it("does the same when it is deleted", async () => {
        const api = createMockApi();
        const { project } = await onScreen(api);
        await useStore.getState().deleteSession(api, 4);
        expect(centre()).toEqual({ selectedSessionId: null, draftProject: project });
    });

    it("is left as soon as its task's removal starts, with its conversation still there", async () => {
        const api = createMockApi();
        const { project, task } = await onScreen(api);
        useStore.getState().leaveForRemoval({ project: "fsum", task: "anything" });
        expect(centre().selectedSessionId).toBe(4);

        useStore.getState().leaveForRemoval({ project, task });
        expect(centre()).toEqual({ selectedSessionId: null, draftProject: project });
        expect(useStore.getState().events[4]?.length).toBeGreaterThan(0);
    });

    it("goes to another repository when its own is being removed", async () => {
        const api = createMockApi();
        const { project } = await onScreen(api);
        useStore.getState().leaveForRemoval({ project, task: null });
        const { selectedSessionId, draftProject } = centre();
        expect(selectedSessionId).toBeNull();
        expect(draftProject).not.toBe(project);
        expect(useStore.getState().projects.map((each) => each.name)).toContain(draftProject);
    });

    it("never stays on screen with its conversation forgotten", async () => {
        const api = createMockApi();
        const { project } = await onScreen(api);
        const taskId = homeOf(4)?.task.id ?? -1;
        useStore.getState().forgetTasks([taskId]);
        expect(centre()).toEqual({ selectedSessionId: null, draftProject: project });
    });

    it("is left when the tree is read again without it", async () => {
        const api = createMockApi();
        const { project, task } = await onScreen(api);
        await api.taskRemove(project, task, true);
        await useStore.getState().refresh(api);
        expect(centre()).toEqual({ selectedSessionId: null, draftProject: project });
    });

    it("is left when another window or an agent closes or deletes it", async () => {
        const base = createMockApi();
        let heard: (event: SessionEvent) => void = () => {};
        const api: Api = {
            ...base,
            onSessionEvent: async (listener) => {
                heard = listener;
                return () => {};
            },
        };
        const disconnect = connect(api);
        const { project } = await onScreen(api);
        await base.sessionClose(4);
        heard(published(4, { type: "closed" }));
        expect(centre()).toEqual({ selectedSessionId: null, draftProject: project });

        useStore.setState({ draftProject: "fsum" });
        await useStore.getState().open(api, 3);
        const other = homeOf(3)?.project.name;
        await base.sessionDelete(3);
        heard(published(3, { type: "deleted" }));
        expect(centre()).toEqual({ selectedSessionId: null, draftProject: other });
        disconnect();
    });
});

describe("a session's queue", () => {
    const waiting = [{ id: 1, text: "/code-review #411" }];
    const answered = () => {
        let answer: (messages: typeof waiting) => void = () => undefined;
        const promise = new Promise<typeof waiting>((resolve) => {
            answer = resolve;
        });
        return { promise, answer };
    };

    beforeEach(() => {
        useStore.setState({ queues: {}, queuesHeard: {} });
    });

    it("takes what the core answers it holds", async () => {
        const read = answered();
        const taking = useStore.getState().queueAnswer(7, read.promise);
        read.answer(waiting);
        await taking;
        expect(useStore.getState().queues[7]).toEqual(waiting);
    });

    it("keeps a change heard before the answer came, which is as new or newer", async () => {
        // Read as the session opens, while its task's setup sends the prompt that waited.
        const read = answered();
        const taking = useStore.getState().queueAnswer(7, read.promise);
        useStore.getState().apply(published(7, { type: "queue", messages: [] }));
        read.answer(waiting);
        await taking;
        expect(useStore.getState().queues[7]).toEqual([]);
    });

    it("is weighed against changes of its own session only", async () => {
        const read = answered();
        const taking = useStore.getState().queueAnswer(7, read.promise);
        useStore.getState().apply(published(8, { type: "queue", messages: [] }));
        read.answer(waiting);
        await taking;
        expect(useStore.getState().queues[7]).toEqual(waiting);
    });
});

describe("unsent messages", () => {
    const drafts = () => useStore.getState().drafts;
    beforeEach(() => useStore.setState({ drafts: {}, newTaskDrafts: {}, recalls: {} }));

    it("are kept per session, and opening another session changes none", async () => {
        const api = createMockApi();
        await useStore.getState().refresh(api);
        await useStore.getState().open(api, 4);
        useStore.getState().editDraft(4, { text: "half a thought" });
        useStore.getState().editDraft(4, { files: ["/tmp/shot.png"] });
        await useStore.getState().open(api, 3);
        useStore.getState().editDraft(3, { text: "another" });
        useStore.getState().startNew();
        await useStore.getState().open(api, 4);
        expect(drafts()).toEqual({
            4: { text: "half a thought", files: ["/tmp/shot.png"], images: [] },
            3: { text: "another", files: [], images: [] },
        });
    });

    it("go back into their own session when a send fails, unless it was written to since", () => {
        useStore.getState().editDraft(4, { text: "send me", files: ["/a"] });
        const unsent = drafts()[4] ?? NO_DRAFT;
        // Sending empties the box; the user moves on to session 3 while it fails.
        useStore.getState().editDraft(4, { text: "", files: [] });
        useStore.setState({ selectedSessionId: 3 });
        useStore.getState().putDraftBack(4, unsent);
        expect(drafts()).toEqual({ 4: { text: "send me", files: ["/a"], images: [] } });

        useStore.getState().editDraft(4, { text: "something new", files: [] });
        useStore.getState().putDraftBack(4, unsent);
        expect(drafts()[4]).toEqual({ text: "something new", files: [], images: [] });
    });

    it("go with a session that is deleted or closed, and with a removed task's sessions", async () => {
        const api = createMockApi();
        await useStore.getState().refresh(api);
        for (const id of [3, 4, 5]) {
            useStore.getState().editDraft(id, { text: `draft ${id}` });
        }
        await useStore.getState().deleteSession(api, 4);
        expect(Object.keys(drafts())).toEqual(["3", "5"]);
        await useStore.getState().close(api, 5);
        expect(Object.keys(drafts())).toEqual(["3"]);
        const { projects } = useStore.getState();
        const session = findSession(projects, 3);
        const taskId = (session && findSessionHome(projects, session)?.task.id) ?? -1;
        useStore.getState().forgetTasks([taskId]);
        expect(drafts()).toEqual({});
    });

    it("keep what was typed before ↑ while another session is open, and go with the session", async () => {
        const api = createMockApi();
        await useStore.getState().refresh(api);
        const typed = { text: "half a thought", files: [], images: [] };
        const sent = { text: "sent before", files: [], images: [] };
        const browsing = { sent: [sent], at: 0, draft: typed, shown: sent };
        useStore.getState().noteRecall(4, browsing);
        useStore.getState().editDraft(4, sent);
        await useStore.getState().open(api, 3);
        await useStore.getState().open(api, 4);
        expect(useStore.getState().recalls).toEqual({ 4: browsing });

        useStore.getState().noteRecall(5, browsing);
        useStore.getState().noteRecall(5, null);
        expect(Object.keys(useStore.getState().recalls)).toEqual(["4"]);
        await useStore.getState().deleteSession(api, 4);
        expect(useStore.getState().recalls).toEqual({});
        useStore.getState().noteRecall(5, browsing);
        await useStore.getState().close(api, 5);
        expect(useStore.getState().recalls).toEqual({});
    });

    it("keep where each transcript was left, and go with the session, sub-agents' too", async () => {
        const api = createMockApi();
        await useStore.getState().refresh(api);
        useStore.setState({ scrolls: {} });
        for (const id of [3, 4, 5]) {
            await useStore.getState().open(api, id);
        }
        const up = { following: false as const, key: 12, offset: 40 };
        const { keepScroll } = useStore.getState();
        keepScroll(3, "3", up);
        keepScroll(4, "4", up);
        keepScroll(4, "4/toolu_01", { following: true });
        keepScroll(5, "5", up);
        // A transcript whose session is not open, or not there, keeps nothing.
        keepScroll(999, "999", up);
        expect(Object.keys(useStore.getState().scrolls).sort()).toEqual([
            "3",
            "4",
            "4/toolu_01",
            "5",
        ]);
        await useStore.getState().deleteSession(api, 4);
        expect(Object.keys(useStore.getState().scrolls).sort()).toEqual(["3", "5"]);
        await useStore.getState().close(api, 5);
        expect(Object.keys(useStore.getState().scrolls)).toEqual(["3"]);
        // Its transcript goes after a closed session: what it would keep then is not kept.
        keepScroll(5, "5", up);
        expect(Object.keys(useStore.getState().scrolls)).toEqual(["3"]);
    });

    it("are kept per repository in the new-task box, and go with a removed repository", async () => {
        const api = createMockApi();
        await useStore.getState().refresh(api);
        const { editNewTaskDraft } = useStore.getState();
        editNewTaskDraft("storefront", { text: "add a search box" });
        editNewTaskDraft("bonsai-lint", { text: "support Go", files: ["/spec.md"] });
        expect(useStore.getState().newTaskDrafts).toEqual({
            storefront: { text: "add a search box", files: [], images: [] },
            "bonsai-lint": { text: "support Go", files: ["/spec.md"], images: [] },
        });
        await api.projectRemove("bonsai-lint", false);
        await useStore.getState().refresh(api);
        expect(Object.keys(useStore.getState().newTaskDrafts)).toEqual(["storefront"]);
        editNewTaskDraft("storefront", { text: "" });
        expect(useStore.getState().newTaskDrafts).toEqual({});
    });

    it("are filled from an issue, which the new task is linked to until it is taken off", async () => {
        const api = createMockApi();
        await useStore.getState().refresh(api);
        useStore.setState({ newTaskIssues: {}, issuesOf: { project: "storefront", issue: null } });
        const { editNewTaskDraft, startFromIssue, linkIssue } = useStore.getState();
        editNewTaskDraft("storefront", { text: "also on Safari" });
        const url = "https://github.com/example/storefront/issues/428";
        startFromIssue("storefront", { number: 428, title: "Login loops", url });
        const state = useStore.getState();
        // The issues are put away and the new-task view of the repository is on screen.
        expect(state.issuesOf).toBeNull();
        expect(state.selectedSessionId).toBeNull();
        expect(state.draftProject).toBe("storefront");
        const link = state.newTaskIssues.storefront;
        expect(link?.number).toBe(428);
        // What was typed stays, under the issue's prompt.
        expect(state.newTaskDrafts.storefront?.text).toBe(`${link?.prompt}\n\nalso on Safari`);
        // Taken off by hand, and gone with its repository.
        linkIssue("storefront", null);
        expect(useStore.getState().newTaskIssues).toEqual({});
        startFromIssue("bonsai-lint", { number: 88, title: "Support Rust", url });
        await api.projectRemove("bonsai-lint", false);
        await useStore.getState().refresh(api);
        expect(useStore.getState().newTaskIssues).toEqual({});
    });
});

describe("what a task's panel holds", () => {
    beforeEach(() => useStore.setState({ stackChanges: {}, commitDrafts: {} }));

    it("is each task's own: a stack stopping and a commit message stay with their task", () => {
        const store = useStore.getState();
        store.noteStackChange(1, "stopping");
        store.editCommitDraft(1, "Fix the redirect loop");
        store.editCommitDraft(2, "Export the VAT column");
        const { stackChanges, commitDrafts } = useStore.getState();
        expect(stackChanges).toEqual({ 1: "stopping" });
        expect(commitDrafts).toEqual({ 1: "Fix the redirect loop", 2: "Export the VAT column" });

        // Done, or emptied, it is forgotten.
        store.noteStackChange(1, null);
        store.editCommitDraft(2, "");
        expect(useStore.getState().stackChanges).toEqual({});
        expect(useStore.getState().commitDrafts).toEqual({ 1: "Fix the redirect loop" });
    });

    it("goes with a removed task", () => {
        const store = useStore.getState();
        store.noteStackChange(1, "starting");
        store.editCommitDraft(1, "Fix the redirect loop");
        store.editCommitDraft(2, "Export the VAT column");
        store.forgetTasks([1]);
        expect(useStore.getState().stackChanges).toEqual({});
        expect(useStore.getState().commitDrafts).toEqual({ 2: "Export the VAT column" });
    });
});

describe("configuration that is not understood whole", () => {
    it("is said in the window, and the settings that were understood are in use", async () => {
        const problem = "settings.toml: `coming_later` is not something this version knows.";
        const api: Api = { ...createMockApi(), configProblems: async () => [problem] };
        useStore.setState({ problem: null, settings: null });
        const disconnect = connect(api);
        await new Promise((resolve) => setTimeout(resolve, 0));
        expect(useStore.getState().problem).toBe(configProblemsSaid([problem]));
        expect(useStore.getState().problem).toContain("`coming_later`");
        expect(useStore.getState().settings).not.toBeNull();
        disconnect();
    });
});

describe("what another Swarmed process writes", () => {
    const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

    it("has the window read the tree again, and is kept in no transcript", async () => {
        let heard: (event: SessionEvent) => void = () => {};
        let refreshes = 0;
        const base = createMockApi();
        const api: Api = {
            ...base,
            overview: async () => {
                refreshes += 1;
                return base.overview();
            },
            onSessionEvent: async (listener) => {
                heard = listener;
                return () => {};
            },
        };
        const disconnect = connect(api);
        await settle();
        const before = refreshes;
        heard(published(0, { type: "tree_changed" }));
        expect(refreshes).toBe(before + 1);
        expect(useStore.getState().events[0]).toBeUndefined();
        disconnect();
    });

    it("has the configuration read again, and keeps what did not change as it was", async () => {
        let heard: (event: SessionEvent) => void = () => {};
        let problems = ["settings.toml: `later` is not something this version knows."];
        const base = createMockApi();
        const api: Api = {
            ...base,
            configProblems: async () => problems,
            onSessionEvent: async (listener) => {
                heard = listener;
                return () => {};
            },
        };
        useStore.setState({ problem: null, settings: null, terminalConfig: null });
        const disconnect = connect(api);
        await settle();
        const { settings, terminalConfig, actionsChanged } = useStore.getState();
        expect(settings).not.toBeNull();
        // Seen and put away: the same problems are not said again.
        useStore.getState().dismissProblem();

        heard(published(0, { type: "config_changed", kind: "file" }));
        await settle();
        expect(useStore.getState().settings).toBe(settings);
        expect(useStore.getState().terminalConfig).toBe(terminalConfig);
        expect(useStore.getState().actionsChanged).toBe(actionsChanged + 1);
        expect(useStore.getState().problem).toBeNull();
        expect(useStore.getState().events[0]).toBeUndefined();

        problems = ["agents.toml: `soon` is not something this version knows."];
        heard(published(0, { type: "config_changed", kind: "file" }));
        await settle();
        expect(useStore.getState().problem).toContain("`soon`");
        disconnect();
    });
});

describe("the account's usage", () => {
    const usage = (used: number): AccountUsage => ({
        windows: [
            {
                kind: "seven_day",
                name: "weekly",
                used,
                resets_at: null,
                status: "allowed",
                heard_at: 100,
                asked: false,
            },
        ],
        extra: null,
        billed: null,
    });

    it("is read once connected, and follows what any session hears, for every session", async () => {
        let heard: (event: SessionEvent) => void = () => {};
        let refreshes = 0;
        const base = createMockApi();
        const api: Api = {
            ...base,
            accountUsage: async () => usage(0.4),
            overview: async () => {
                refreshes += 1;
                return base.overview();
            },
            onSessionEvent: async (listener) => {
                heard = listener;
                return () => {};
            },
        };
        useStore.setState({ accountUsage: null });
        const disconnect = connect(api);
        await new Promise((resolve) => setTimeout(resolve, 0));
        expect(useStore.getState().accountUsage).toEqual(usage(0.4));
        const before = refreshes;

        heard(published(0, { type: "account_usage", usage: usage(0.88) }));
        expect(useStore.getState().accountUsage).toEqual(usage(0.88));
        // It belongs to no session: nothing is read again for it, and no transcript holds it.
        expect(refreshes).toBe(before);
        expect(useStore.getState().events[0]).toBeUndefined();
        disconnect();
    });

    it("is not put back by a read that an event overtook", async () => {
        let heard: (event: SessionEvent) => void = () => {};
        let answer: (usage: AccountUsage) => void = () => {};
        const api: Api = {
            ...createMockApi(),
            accountUsage: () => new Promise((resolve) => (answer = resolve)),
            onSessionEvent: async (listener) => {
                heard = listener;
                return () => {};
            },
        };
        useStore.setState({ accountUsage: null });
        const disconnect = connect(api);
        await new Promise((resolve) => setTimeout(resolve, 0));
        heard(published(0, { type: "account_usage", usage: usage(0.88) }));
        answer(usage(0.4));
        await new Promise((resolve) => setTimeout(resolve, 0));
        expect(useStore.getState().accountUsage).toEqual(usage(0.88));
        disconnect();
    });
});

describe("the queue of waiting messages", () => {
    it("is kept beside the transcript and follows the core's events", () => {
        useStore.setState({ events: { 1: [text("a")] }, queues: {} });
        const waiting = [{ id: 7, text: "next" }];
        useStore.getState().apply(published(1, { type: "queue", messages: waiting }));
        expect(useStore.getState().queues[1]).toEqual(waiting);
        expect(useStore.getState().events[1]).toEqual([text("a")]);
    });

    it("follows the mock through a rewrite and an emptying, as it would the core", async () => {
        const api = createMockApi();
        const disconnect = connect(api);
        const queued = () => useStore.getState().queues[3]?.map((message) => message.text);
        const [first] = await api.sessionEnqueue(3, "1st");
        await api.sessionEnqueue(3, "second");
        expect(queued()).toEqual(["1st", "second"]);
        await api.sessionQueueEdit(3, first?.id ?? 0, "first");
        expect(queued()).toEqual(["first", "second"]);
        await api.sessionQueueEdit(3, first?.id ?? 0, " ");
        expect(queued()).toEqual(["second"]);
        disconnect();
    });
});

describe("opening a session", () => {
    it("marks its unread answer as read, here and in the app", async () => {
        const api = createMockApi();
        await useStore.getState().refresh(api);
        const unread = () =>
            useStore
                .getState()
                .projects.flatMap((project) => project.tasks)
                .flatMap((task) => task.sessions)
                .find((session) => session.id === 5)?.unread;
        expect(unread()).toBe(true);
        await useStore.getState().open(api, 5);
        expect(unread()).toBe(false);
        const stored = (await api.overview())
            .flatMap((project) => project.tasks)
            .flatMap((task) => task.sessions)
            .find((session) => session.id === 5);
        expect(stored?.unread).toBe(false);
    });
});

describe("a folded task", () => {
    const stored = new Map<string, string>();
    const remembered = () => stored.get("swarmed.folded-tasks");
    beforeEach(() => {
        stored.clear();
        vi.stubGlobal("localStorage", {
            getItem: (key: string) => stored.get(key) ?? null,
            setItem: (key: string, value: string) => stored.set(key, value),
        });
        useStore.setState({ foldedTasks: [] });
    });
    afterEach(() => vi.unstubAllGlobals());

    it("folds and unfolds by id, and is remembered on this machine", () => {
        useStore.getState().toggleTaskFolded(4);
        useStore.getState().toggleTaskFolded(1);
        expect(useStore.getState().foldedTasks).toEqual([4, 1]);
        expect(remembered()).toBe("[4,1]");
        useStore.getState().toggleTaskFolded(4);
        expect(remembered()).toBe("[1]");
    });

    it("unfolds when one of its sessions is opened, from wherever that was", async () => {
        const api = createMockApi();
        await useStore.getState().refresh(api);
        useStore.setState({ foldedTasks: [1, 4] });
        await useStore.getState().open(api, 5);
        expect(useStore.getState().foldedTasks).toEqual([1]);
        expect(remembered()).toBe("[1]");
    });

    it("stays folded over the session on screen when that one is only read again", async () => {
        const api = createMockApi();
        await useStore.getState().refresh(api);
        await useStore.getState().open(api, 5);
        // Folded by the user with its session open, then read again after events were missed.
        useStore.setState({ foldedTasks: [4] });
        await useStore.getState().open(api, 5);
        expect(useStore.getState().foldedTasks).toEqual([4]);
    });

    it("unfolds when a session is started in it", async () => {
        await useStore.getState().refresh(moduleApi);
        useStore.setState({ foldedTasks: [4] });
        await newSession("bonsai-lint", "rust-support");
        expect(useStore.getState().selectedSessionId).not.toBeNull();
        expect(useStore.getState().foldedTasks).toEqual([]);
    });

    it("is forgotten with its task, removed here or anywhere else", async () => {
        const api = createMockApi();
        await useStore.getState().refresh(api);
        useStore.setState({ foldedTasks: [1, 2, 4] });
        useStore.getState().forgetTasks([4]);
        expect(useStore.getState().foldedTasks).toEqual([1, 2]);

        // Removed from another window or the command line: the tree read again lacks it.
        await api.taskRemove("storefront", "fix-login-redirect", true);
        await useStore.getState().refresh(api);
        expect(useStore.getState().foldedTasks).toEqual([2]);
        expect(remembered()).toBe("[2]");
    });
});

describe("setting up an agent", () => {
    it("tells the menus to read the agents again", () => {
        const before = useStore.getState().agentsVersion;
        useStore.getState().agentsChanged();
        expect(useStore.getState().agentsVersion).toBe(before + 1);
    });
});

describe("the settings", () => {
    it("open on the part asked for, and otherwise where they were last", () => {
        useStore.getState().showSettings(true, "shortcuts");
        expect(useStore.getState().settingsTab).toBe("shortcuts");
        useStore.getState().showSettings(false);
        useStore.getState().showSettings(true);
        expect(useStore.getState()).toMatchObject({ settingsOpen: true, settingsTab: "shortcuts" });
        useStore.getState().showSettings(false, "appearance");
    });
});

describe("closed sessions", () => {
    it("are counted on their task, listed, and come back when reopened", async () => {
        const api = createMockApi();
        await useStore.getState().refresh(api);
        await useStore.getState().close(api, 3);
        const task = () =>
            useStore
                .getState()
                .projects.flatMap((project) => project.tasks)
                .find((candidate) => candidate.name === "invoice-export");
        expect(task()?.closed).toBe(1);
        expect(task()?.sessions).toEqual([]);
        const closed = await api.sessionsClosed("storefront", "invoice-export");
        expect(closed.map((session) => session.id)).toEqual([3]);

        await api.sessionReopen(3);
        await useStore.getState().refresh(api);
        expect(task()?.closed).toBe(0);
        expect(task()?.sessions.map((session) => session.id)).toEqual([3]);
    });
});

describe("runInTerminal", () => {
    it("shows the terminal and hands the command over once, to the session it is for", () => {
        useStore.setState({ terminalOpen: false, terminalCommand: null });
        useStore.getState().runInTerminal(7, "make test");
        expect(useStore.getState().terminalOpen).toBe(true);
        // Another session's terminal leaves it waiting.
        expect(useStore.getState().takeTerminalCommand(8)).toBeNull();
        expect(useStore.getState().takeTerminalCommand(7)).toBe("make test");
        // Asked again, as an effect that runs twice does: nothing left to type.
        expect(useStore.getState().takeTerminalCommand(7)).toBeNull();
    });

    it("shows the shell in place of a Claude Code shown there, so the command runs at once", () => {
        useStore.setState({ terminalCommand: null, claudeTerminals: { 3: "vim", 7: "vim" } });
        useStore.getState().runInTerminal(7, "make deploy");
        // Left waiting behind Claude Code, it would run whenever the shell was next shown.
        expect(useStore.getState().claudeTerminals).toEqual({ 3: "vim" });
        expect(useStore.getState().takeTerminalCommand(7)).toBe("make deploy");
    });
});

describe("Claude Code opened in a session's terminal", () => {
    it("shows the terminal with it, for that session only, until the shell is shown again", () => {
        useStore.setState({ terminalOpen: false, claudeTerminals: { 3: "vim" } });
        useStore.getState().openClaudeTerminal(7, "privacy-settings");
        expect(useStore.getState().terminalOpen).toBe(true);
        expect(useStore.getState().claudeTerminals).toEqual({ 3: "vim", 7: "privacy-settings" });
        useStore.getState().leaveClaudeTerminal(7);
        expect(useStore.getState().claudeTerminals).toEqual({ 3: "vim" });
    });
});

describe("the menu bar", () => {
    beforeEach(() => {
        useStore.setState({ banner: null, settingsOpen: false });
    });

    it("opens the About view, the shortcuts and the settings for its own items", () => {
        menuChosen("about");
        expect(useStore.getState().banner).toBe("about");
        menuChosen("shortcuts");
        expect(useStore.getState().banner).toBe("shortcuts");
        menuChosen("settings");
        expect(useStore.getState().settingsOpen).toBe(true);
    });

    it("does nothing for an item it does not know", () => {
        menuChosen("quit");
        expect(useStore.getState()).toMatchObject({ banner: null, settingsOpen: false });
    });

    it("reads the installed version when it connects", async () => {
        useStore.setState({ version: null });
        const stop = connect(createMockApi());
        await expect.poll(() => useStore.getState().version).toBe("0.1.0");
        stop();
    });
});

describe("a picked element's token", () => {
    it("goes to the box of the session on screen, and to another's draft at its end", () => {
        useStore.setState({
            selectedSessionId: 4,
            drafts: { 5: { text: "see", files: [], images: [] } },
        });
        const { placePick, takePick } = useStore.getState();
        placePick(4, "[Element 1: li]");
        expect(takePick(5)).toBeNull();
        expect(takePick(4)).toBe("[Element 1: li]");
        expect(takePick(4)).toBeNull();
        placePick(5, "[Element 2: a]");
        expect(useStore.getState().drafts[5]?.text).toBe("see [Element 2: a] ");
    });

    it("is no longer asked for once the Browser pane is closed", () => {
        const { toggleBrowser, askToPick } = useStore.getState();
        toggleBrowser(true);
        askToPick(true);
        toggleBrowser(true);
        expect(useStore.getState().pickAsked).toBe(true);
        toggleBrowser(false);
        expect(useStore.getState().pickAsked).toBe(false);
    });
});

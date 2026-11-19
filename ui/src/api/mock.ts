import type { Api } from "./client";
import type { Project, Session, SessionEvent, SessionStatus, Task } from "./types";

// Sample data for working on the UI in a browser, where there is no Tauri shell.

let nextSessionId = 1;

function session(taskId: number, title: string, status: SessionStatus): Session {
    const id = nextSessionId++;
    return {
        id,
        task_id: taskId,
        title,
        agent_session_id: `mock-${id}`,
        model: null,
        supervision: "ask",
        status,
    };
}

function task(id: number, projectId: number, name: string, sessions: Session[]): Task {
    return {
        id,
        project_id: projectId,
        name,
        branch: `ricardas/${name}`,
        base_ref: "origin/main",
        worktree_path: `/Users/you/swarmed/hero/${name}`,
        slot: id,
        setup: "ok",
        archived: false,
        sessions,
        running: sessions.filter((s) => s.status !== "idle").map((s) => s.id),
    };
}

export function sampleProjects(): Project[] {
    nextSessionId = 1;
    return [
        {
            id: 1,
            name: "hero",
            repo_path: "/Users/you/code/hero",
            tasks: [
                task(1, 1, "fix-login-redirect", [
                    session(1, "Fix the redirect loop after login", "waiting"),
                    session(1, "Write a regression test for the loop", "working"),
                ]),
                task(2, 1, "invoice-export", [
                    session(2, "Why does the CSV export drop the VAT column?", "idle"),
                ]),
                task(3, 1, "pm-16037", []),
            ],
        },
        {
            id: 2,
            name: "bonsai-lint",
            repo_path: "/Users/you/code/bonsai-lint",
            tasks: [
                task(4, 2, "rust-support", [session(4, "Sketch a Rust language spec", "errored")]),
            ],
        },
        { id: 3, name: "fsum", repo_path: "/Users/you/code/fsum", tasks: [] },
    ];
}

type Listener = (event: SessionEvent) => void;

/** An in-memory stand-in. The second sample session finishes its turn after a few seconds. */
export function createMockApi(): Api {
    const listeners = new Set<Listener>();
    setTimeout(() => {
        const done: SessionEvent = {
            session_id: 2,
            status: "idle",
            event: { type: "turn_done", is_error: false, result: null, cost_usd: 0.02 },
        };
        for (const listener of listeners) {
            listener(done);
        }
    }, 6000);
    return {
        overview: async () => sampleProjects(),
        onSessionEvent: async (listener) => {
            listeners.add(listener);
            return () => listeners.delete(listener);
        },
        onEventsLagged: async () => () => undefined,
    };
}

// Taking a sample session back to before one of its messages, as the core does (`rewind.rs`):
// refused while it works, its files refused while another session of its task works or in a
// chat, and the files a rewind took back put back once.

import { droppedRanges } from "../rewind";
import type { Api } from "./client";
import { sampleTurnFiles } from "./mockTurns";
import type { AgentEvent, Project, RewindCheck, RewindDone, Session } from "./types";

/** What taking a sample session back needs of the rest of the mock. */
export interface RewindHost {
    projects(): Project[];
    history(id: number): AgentEvent[];
    publish(id: number, event: AgentEvent): void;
    /** Stops a session's agent: it goes on from the point chosen when it starts again. */
    stop(id: number): void;
}

const BUSY = new Set(["working", "waiting"]);

/** The session `id`, its task's other sessions, and whether it is a chat. */
function placeOf(host: RewindHost, id: number) {
    for (const project of host.projects()) {
        for (const task of project.tasks) {
            const session = task.sessions.find((one) => one.id === id);
            if (session) {
                return { session, sessions: task.sessions, chat: project.chats };
            }
        }
    }
    throw new Error(`session ${id} not found`);
}

/** Why the conversation cannot go back to before message `message`, if it cannot. */
function refusal(host: RewindHost, session: Session, message: number): string | null {
    if (BUSY.has(session.status)) {
        return "the session is at work: stop its turn first";
    }
    const events = host.history(session.id);
    const messages = events.flatMap((event, at) => (event.type === "user_message" ? [at] : []));
    const at = messages[message];
    const dropped = droppedRanges(events).some(
        ([start, end]) => at !== undefined && at >= start && at < end,
    );
    if (at === undefined || dropped) {
        return `message ${message} is not in the conversation: it was never sent, or a rewind dropped it already`;
    }
    return null;
}

/** Why its files cannot go back with it, if they cannot. */
function filesRefusal(place: ReturnType<typeof placeOf>): string | null {
    if (place.chat) {
        return "a chat has no repository: start a task in a repository for that";
    }
    const busy = place.sessions.find((one) => BUSY.has(one.status));
    return busy
        ? `“${busy.title}” is at work in this worktree: wait for it, or stop it first`
        : null;
}

/** What taking sample session `id` back to before its message `message` would do now. */
function checkOf(host: RewindHost, id: number, message: number): RewindCheck {
    const place = placeOf(host, id);
    const refused = refusal(host, place.session, message);
    const resumes = place.session.agent === "claude" && message > 0;
    const files_refused = refused === null ? filesRefusal(place) : null;
    const allowed = refused === null && files_refused === null;
    const files = allowed ? sampleTurnFiles(id, message) : [];
    // One sample session committed during its turns.
    const commits_since = allowed ? (id === 1 ? 1 : 0) : null;
    return { refused, files_refused, resumes, files, commits_since };
}

/** Takes sample session `id` back to before its message `message`, its files too with `files`. */
function rewindOne(host: RewindHost, id: number, message: number, files: boolean): RewindDone {
    const looked = checkOf(host, id, message);
    const reason = looked.refused ?? (files ? looked.files_refused : null);
    if (reason !== null) {
        throw new Error(reason);
    }
    host.stop(id);
    const commits_since = files ? looked.commits_since : null;
    const resume_at = looked.resumes ? `sample-point-${message}` : null;
    host.publish(id, { type: "rewound", to: message, files, resume_at, commits_since });
    return { to: message, files, resumes: looked.resumes, commits_since };
}

/** The sample sessions taken back, as the core takes them back. */
export function rewindApi(
    host: RewindHost,
): Pick<Api, "sessionRewindCheck" | "sessionRewind" | "sessionFilesPutBack"> {
    // The rewinds whose files can still be put back, by session: the message each was to.
    const kept = new Map<number, number>();
    return {
        sessionRewindCheck: async (id, message) => checkOf(host, id, message),
        sessionRewind: async (id, message, files) => {
            const done = rewindOne(host, id, message, files);
            if (files) {
                kept.set(id, message);
            }
            return done;
        },
        sessionFilesPutBack: async (id) => {
            const to = kept.get(id);
            if (to === undefined) {
                throw new Error("no files this session took back can be put back");
            }
            const refused = filesRefusal(placeOf(host, id));
            if (refused !== null) {
                throw new Error(refused);
            }
            kept.delete(id);
            host.publish(id, { type: "files_put_back", to });
        },
    };
}

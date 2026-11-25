import { CHATS } from "../chats";
import { type SampleElement, withElementList } from "../elements";
import { withGitHubList } from "../githubMentions";
import { type Mentioned, withMentions } from "../mentions";
import { withFiles } from "../transcript";
import type { Api } from "./client";
import { githubFor } from "./mockGithub";
import type { Project, Reachable, Session, Task } from "./types";

// Which sessions a message can mention in the sample data, and the message it is sent as, as
// the core works them out (`peers.rs`, `mentions.rs`): a repository's session reads the
// sessions of its own repository, a chat every open one, and none reads a chat from a
// repository.

/** A session with where it works. */
interface Placed {
    session: Session;
    task: Task;
    project: Project;
}

function placed(projects: Project[]): Placed[] {
    return projects.flatMap((project) =>
        project.tasks
            .filter((task) => !task.archived)
            .flatMap((task) => task.sessions.map((session) => ({ session, task, project }))),
    );
}

/** The sessions a session of `project` can read, but `own`, those of task `task` first, the latest first. */
function reachableFrom(
    projects: Project[],
    project: Project | undefined,
    own: { session: number; task: number } | null,
): Reachable[] {
    const everywhere = project === undefined || project.chats;
    const read = placed(projects).filter(
        (one) => (everywhere || one.project.id === project.id) && one.session.id !== own?.session,
    );
    const listed = read.map(({ session, task, project: home }) => ({
        id: session.id,
        title: session.title,
        task: task.name,
        project: home.name,
        chat: home.chats,
        same_task: task.id === own?.task,
        status: session.status,
    }));
    return listed.sort((a, b) => Number(b.same_task) - Number(a.same_task) || b.id - a.id);
}

/** The sessions session `id` can mention. */
function reachableBy(projects: Project[], id: number): Reachable[] {
    const own = placed(projects).find((one) => one.session.id === id);
    if (!own) {
        throw new Error(`session ${id} not found`);
    }
    return reachableFrom(projects, own.project, { session: id, task: own.task.id });
}

/** The sessions a new task's first message in `project` can mention. */
function reachableIn(projects: Project[], project: string): Reachable[] {
    const home = projects.find((one) => one.name === project);
    if (project !== CHATS && !home) {
        throw new Error(`project \`${project}\` not found`);
    }
    return reachableFrom(projects, project === CHATS ? undefined : home, null);
}

/** The sessions `ids` names, each one of `reachable`, or the core's refusal of the first that is not. */
function checked(reachable: Reachable[], ids: number[]): Mentioned[] {
    return [...new Set(ids)].map((id) => {
        const found = reachable.find((session) => session.id === id);
        if (!found) {
            throw new Error(
                `session ${id} cannot be mentioned here: it is not another open session this one can read`,
            );
        }
        return { id, title: found.title };
    });
}

/**
 * The message session `id` is sent: the words, the sessions mentioned, the pull requests and
 * issues, the elements picked in the task's browser, then the files.
 */
export function messageFor(
    projects: Project[],
    id: number,
    text: string,
    files: string[] = [],
    sessions: number[] = [],
    github: number[] = [],
    elements: SampleElement[] = [],
): string {
    const mentioned = sessions.length === 0 ? [] : checked(reachableBy(projects, id), sessions);
    const home = placed(projects).find((one) => one.session.id === id)?.project.name ?? "";
    const listed = github.length === 0 ? [] : githubFor(home, github);
    const words = withGitHubList(withMentions(text, mentioned), listed);
    return withFiles(withElementList(words, elements), files);
}

/** The first message of a new task in `project`, checked before the task is made. */
export function firstMessageIn(
    projects: Project[],
    project: string,
    text: string,
    sessions: number[],
    github: number[] = [],
): string {
    const mentioned =
        sessions.length === 0 ? [] : checked(reachableIn(projects, project), sessions);
    const listed = github.length === 0 ? [] : githubFor(project, github);
    return withGitHubList(withMentions(text, mentioned), listed);
}

/** The two lists, from the sample's tree. */
export function mentionsApi(
    projects: () => Project[],
): Pick<Api, "sessionReachable" | "projectReachable"> {
    return {
        sessionReachable: async (id) => reachableBy(projects(), id),
        projectReachable: async (project) => reachableIn(projects(), project),
    };
}

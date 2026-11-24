// The chats: sessions with no repository. The core keeps them as the tasks of a project of
// their own, under a name no repository can have, and the window lists them apart.

import type { Project } from "./api/types";
import type { Words } from "./wording";

/**
 * The name the core keeps the chats under (`CHATS` in `chats.rs`). A repository's name
 * starts with a letter or a digit, so none is ever called this; what the window calls the
 * chats is a word of the style, never this name.
 */
export const CHATS = ".chats";

/** Where the chats are kept, once there is a chat. */
export function chatsOf(projects: Project[]): Project | undefined {
    return projects.find((project) => project.chats);
}

/** The repositories, without the chats. */
export function repositoriesOf(projects: Project[]): Project[] {
    return projects.filter((project) => !project.chats);
}

/** Whether `project`, by name, is where the chats are kept. */
export function isChats(project: string): boolean {
    return project === CHATS;
}

/**
 * The repository a new task starts in: the one picked, else the one the view was opened for,
 * such as by a repository's plus. `null` for no repository, which is where New task starts,
 * and for a repository that is gone.
 */
export function newTaskRepository(
    projects: Project[],
    picked: string | null,
    openedFor: string | null,
): Project | null {
    const name = picked ?? openedFor;
    const found = repositoriesOf(projects).find((project) => project.name === name);
    return found ?? null;
}

/** The repository picker's choices: no repository first, then each repository. */
export function repositoryChoices(projects: Project[], none: string): [string, string][] {
    const repositories = repositoriesOf(projects).map((project): [string, string] => [
        project.name,
        project.name,
    ]);
    return [[CHATS, none], ...repositories];
}

/** What a project is called where it is shown: a repository's name, or the chats' heading. */
export function placeName(project: string, words: Words): string {
    return isChats(project) ? words.chats : project;
}

/** Where a new task goes: its repository, if it has one, and the name the core is asked for. */
export interface NewTaskPlace {
    repository: Project | null;
    /** The repository's name, or `CHATS`. */
    home: string;
    /** The names of the tasks already there, or of the chats. */
    taken: string[];
}

/** Where a new task goes, as `newTaskRepository` picks its repository. */
export function newTaskPlace(
    projects: Project[],
    picked: string | null,
    openedFor: string | null,
): NewTaskPlace {
    const repository = newTaskRepository(projects, picked, openedFor);
    const there = repository ?? chatsOf(projects);
    return {
        repository,
        home: repository?.name ?? CHATS,
        taken: there?.tasks.map((task) => task.name) ?? [],
    };
}

/** What the new-task view says in its box and under it: for a task, or for a chat. */
export function newTaskSays(words: Words, repository: Project | null) {
    return repository
        ? { placeholder: words.describeTask, hint: words.newTaskHint }
        : { placeholder: words.describeChat, hint: words.newChatHint };
}

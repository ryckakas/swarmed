import { describe, expect, it } from "vitest";
import { sampleProjects } from "./api/mock";
import type { Project } from "./api/types";
import {
    branchAsked,
    CHATS,
    chatsOf,
    inSidebarOrder,
    isChats,
    newTaskPlace,
    newTaskRepository,
    newTaskSays,
    placeName,
    repositoriesOf,
    repositoryChoices,
} from "./chats";
import { wordsFor } from "./wording";

const projects = sampleProjects();
const words = wordsFor("plain");

/** The sample without its chats, as a window shows it before the first chat is made. */
const noChats = repositoriesOf(projects);

/** A repository named what the window calls the chats. */
function general(): Project {
    return {
        id: 99,
        name: "general",
        repo_path: "/Users/you/code/general",
        chats: false,
        tasks: [],
        has_note: false,
        cloud: [],
        pinned: false,
    };
}

describe("the chats and the repositories", () => {
    it("are told apart by the flag, not by what the window calls them", () => {
        const all = [...projects, general()];
        expect(chatsOf(all)?.name).toBe(CHATS);
        expect(repositoriesOf(all).map((one) => one.name)).toEqual([
            "storefront",
            "bonsai-lint",
            "fsum",
            "general",
        ]);
        expect(chatsOf(noChats)).toBeUndefined();
        expect(isChats(CHATS)).toBe(true);
        expect(isChats("general")).toBe(false);
    });

    it("are listed with the chats first, wherever the core keeps them", () => {
        // The sample keeps the chats last, as the core does a home made after a repository.
        expect(projects.at(-1)?.chats).toBe(true);
        expect(inSidebarOrder(projects).map((one) => one.name)).toEqual([
            CHATS,
            "storefront",
            "bonsai-lint",
            "fsum",
        ]);
        expect(inSidebarOrder(noChats)).toEqual(noChats);
    });

    it("ask for an existing branch only in a repository", () => {
        const repository = noChats[0] ?? null;
        expect(branchAsked(repository, "main", "new")).toBe("main");
        expect(branchAsked(repository, "new", "new")).toBeNull();
        // One picked for a repository before is not sent for a chat.
        expect(branchAsked(null, "main", "new")).toBeNull();
    });

    it("show the chats under the style's heading, and a repository by its name", () => {
        expect(placeName(CHATS, words)).toBe("General");
        expect(placeName("general", words)).toBe("general");
        expect(placeName(CHATS, wordsFor("colony"))).toBe("Commons");
    });
});

describe("where a new task goes", () => {
    it("is no repository unless one was picked or the view was opened for one", () => {
        expect(newTaskRepository(projects, null, null)).toBeNull();
        expect(newTaskRepository(projects, null, "bonsai-lint")?.name).toBe("bonsai-lint");
        expect(newTaskRepository(projects, "fsum", "bonsai-lint")?.name).toBe("fsum");
        // Picking no repository, or opening the view from a chat, is a chat.
        expect(newTaskRepository(projects, CHATS, "bonsai-lint")).toBeNull();
        expect(newTaskRepository(projects, null, CHATS)).toBeNull();
        // A repository that is gone leaves none picked.
        expect(newTaskRepository(projects, null, "removed")).toBeNull();
    });

    it("takes the names of the tasks there, or of the chats", () => {
        const chat = newTaskPlace(projects, null, null);
        expect(chat).toEqual({ repository: null, home: CHATS, taken: ["what-is-a-monad"] });
        // Before the first chat there are none to step past.
        expect(newTaskPlace(noChats, null, null).taken).toEqual([]);
        const task = newTaskPlace(projects, null, "storefront");
        expect(task.home).toBe("storefront");
        expect(task.taken).toContain("fix-login-redirect");
        // A repository called what the chats are called is a repository.
        const named = newTaskPlace([...projects, general()], "general", null);
        expect(named.repository?.name).toBe("general");
        expect(named.taken).toEqual([]);
    });

    it("offers no repository first in the picker, then each repository", () => {
        expect(repositoryChoices(projects, words.noRepository)).toEqual([
            [CHATS, "No repository"],
            ["storefront", "storefront"],
            ["bonsai-lint", "bonsai-lint"],
            ["fsum", "fsum"],
        ]);
    });

    it("says what sending does for a task and for a chat", () => {
        expect(newTaskSays(words, null)).toEqual({
            placeholder: "Ask anything",
            hint: words.newChatHint,
        });
        const storefront = newTaskRepository(projects, "storefront", null);
        expect(newTaskSays(words, storefront).hint).toBe(words.newTaskHint);
    });
});

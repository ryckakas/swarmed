import { useEffect, useState } from "react";
import { api } from "../api";
import type { GitHubItem, GitHubItems } from "../api/types";
import {
    asksGitHub,
    githubMatches,
    githubOffered,
    githubQuery,
    githubSaid,
    withGitHub,
} from "../githubMentions";
import { useStore } from "../store";
import { usePicking } from "../usePicking";

/** How long typing has to pause before GitHub's search is asked, in milliseconds. */
const SEARCH_AFTER = 350;

/** What the core said for one question, with the question, so that an old answer is not shown. */
interface Answer {
    key: string;
    said: GitHubItems;
}

/**
 * What the core offers for `query` in `project` while `asking`, after `wait` milliseconds:
 * `null` until it has answered. Asked again each time `asking` turns on or the question
 * changes, so that a list opened again is as GitHub has it now.
 */
function useGitHubAnswer(
    project: string | undefined,
    query: string | null,
    wait: number,
): GitHubItems | null {
    const [answer, setAnswer] = useState<Answer | null>(null);
    const key = project === undefined || query === null ? null : `${project}\n${query}`;
    // biome-ignore lint/correctness/useExhaustiveDependencies: asked again only when the question changes
    useEffect(() => {
        if (key === null || project === undefined || query === null) {
            return;
        }
        let current = true;
        const timer = setTimeout(() => {
            api.projectGithubItems(project, query)
                .then((said) => {
                    if (current) {
                        setAnswer({ key, said });
                    }
                })
                .catch(useStore.getState().report);
        }, wait);
        return () => {
            current = false;
            clearTimeout(timer);
        };
    }, [key]);
    return answer && answer.key === key ? answer.said : null;
}

/**
 * The list of pull requests and issues to mention for a `#` being typed in a box holding
 * `value` with the caret at `caret`, or `null` where it is not known or another list has the
 * caret. The repository's latest are read from the core once each time the list opens and
 * filtered as the user types; what they do not hold is asked of GitHub's search once typing
 * pauses. Without a repository (`project`), `#` is only a character.
 */
export function useGitHubMenu(
    value: string,
    caret: number | null,
    project: string | undefined,
    onChange: (value: string, select: [number, number]) => void,
) {
    const picking = usePicking(value);
    const listing = project !== undefined && caret !== null && !picking.away;
    const typed = listing ? githubQuery(value, caret) : null;
    const recent = useGitHubAnswer(project, typed === null ? null : "", 0);
    const query = typed?.query ?? "";
    const local = typed && recent ? githubMatches(recent.items, query) : [];
    const searching = recent !== null && typed !== null && asksGitHub(query, local);
    const searched = useGitHubAnswer(project, searching ? query.trim() : null, SEARCH_AFTER);
    const matches = searching ? githubOffered(local, searched?.items ?? null) : local;
    const note = typed === null ? null : noteFor(recent, searching, searched, matches);
    const choose = (item: GitHubItem) => {
        if (!typed) {
            return;
        }
        picking.restart();
        const end = typed.start + 1 + typed.query.length;
        const placed = withGitHub(value, typed.start, end, item);
        onChange(placed.value, [placed.caret, placed.caret]);
    };
    const onKeyDown = picking.keysFor(matches, note !== null, choose);
    return { matches, active: picking.activeOf(matches.length), note, choose, onKeyDown };
}

/**
 * What the list says under what it shows: that it is reading, why GitHub gave nothing, or that
 * nothing matches. `null` when it shows matches and has nothing to add.
 */
function noteFor(
    recent: GitHubItems | null,
    searching: boolean,
    searched: GitHubItems | null,
    matches: GitHubItem[],
): string | null {
    if (recent === null) {
        return "Reading pull requests and issues…";
    }
    const answer = searching ? searched : recent;
    if (answer === null) {
        return "Searching GitHub…";
    }
    if (answer.unavailable !== null) {
        return answer.unavailable;
    }
    return matches.length === 0 ? "No pull request or issue matches" : null;
}

interface MenuProps {
    menu: ReturnType<typeof useGitHubMenu>;
}

/** The pull requests and issues offered for a `#` being typed, above the box like the command list. */
export function GitHubMenu({ menu }: MenuProps) {
    if (menu.matches.length === 0 && menu.note === null) {
        return null;
    }
    return (
        <ul className="slash-menu mention-menu" aria-label="Pull requests and issues to mention">
            {menu.matches.map((item, index) => (
                <li key={item.number}>
                    <button
                        type="button"
                        className="slash-option"
                        aria-current={index === menu.active ? "true" : undefined}
                        title={`${githubSaid(item)}: ${item.title}`}
                        // Keeps the text box focused, so typing carries on after the click.
                        onMouseDown={(event) => event.preventDefault()}
                        onClick={() => menu.choose(item)}
                    >
                        <span className="github-number" data-state={item.state}>
                            #{item.number}
                        </span>
                        {item.title || "Untitled"}
                        <span className="slash-kind">
                            {item.kind === "pull" ? "pull request" : "issue"}, {item.state}
                        </span>
                    </button>
                </li>
            ))}
            {menu.note !== null && <li className="mention-hint">{menu.note}</li>}
        </ul>
    );
}

import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { SearchHit } from "../api/types";
import { useStore } from "../store";

/** How long typing has to pause before the search is run, in milliseconds. */
const PAUSE_MS = 180;

/** What was found for the words typed so far; `null` while nothing has been asked. */
function useSearch(query: string): SearchHit[] | null {
    const [hits, setHits] = useState<SearchHit[] | null>(null);
    useEffect(() => {
        if (query.trim() === "") {
            setHits(null);
            return;
        }
        let current = true;
        const timer = setTimeout(() => {
            api.sessionsSearch(query).then(
                (found) => current && setHits(found),
                () => current && setHits([]),
            );
        }, PAUSE_MS);
        return () => {
            current = false;
            clearTimeout(timer);
        };
    }, [query]);
    return hits;
}

/** Down from the box goes into the results, and up and down move between them. */
function moveFocus(event: KeyboardEvent<HTMLElement>) {
    const step = { ArrowDown: 1, ArrowUp: -1 }[event.key];
    if (step === undefined) {
        return;
    }
    const stops = [
        ...event.currentTarget.querySelectorAll<HTMLElement>(".search-box, .search-hit"),
    ];
    const next = stops[stops.indexOf(document.activeElement as HTMLElement) + step];
    if (next) {
        event.preventDefault();
        next.focus();
    }
}

/**
 * Finds a conversation again by what was said in it, in any repository, closed sessions
 * included. Choosing one opens it, reopening it first if it was closed.
 */
export function SearchDialog() {
    const open = useStore((state) => state.searchOpen);
    const showSearch = useStore((state) => state.showSearch);
    const openSession = useStore((state) => state.open);
    const refresh = useStore((state) => state.refresh);
    const report = useStore((state) => state.report);
    const dialog = useRef<HTMLDialogElement>(null);
    const [query, setQuery] = useState("");
    const hits = useSearch(open ? query : "");

    // A modal <dialog> keeps the keyboard inside it and closes on Esc by itself.
    useEffect(() => {
        if (open && !dialog.current?.open) {
            dialog.current?.showModal();
        }
    }, [open]);

    if (!open) {
        return null;
    }
    const go = (hit: SearchHit) => {
        showSearch(false);
        // A closed session is put back in the sidebar before it can be shown.
        const ready = hit.closed
            ? api.sessionReopen(hit.session_id).then(() => refresh(api))
            : Promise.resolve();
        ready.then(() => openSession(api, hit.session_id)).catch(report);
    };
    return (
        <dialog
            ref={dialog}
            className="confirm inbox search"
            aria-label="Search conversations"
            onClose={() => showSearch(false)}
            onKeyDown={moveFocus}
        >
            <input
                className="search-box"
                type="search"
                aria-label="Search conversations"
                placeholder="Search what was said, in any session"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
            />
            {hits?.length === 0 && <p>Nobody said that.</p>}
            {hits?.map((hit) => (
                <button
                    key={hit.session_id}
                    type="button"
                    className="search-hit"
                    onClick={() => go(hit)}
                >
                    <span className="search-hit-head">
                        <span className="inbox-title">{hit.title || "New session"}</span>
                        <span className="inbox-place">
                            {hit.project} / {hit.task}
                            {hit.closed && ", closed"}
                            {hit.matches > 1 && `, ${hit.matches} places`}
                        </span>
                    </span>
                    <span className="search-snippet">
                        {hit.by_user ? "You: " : ""}
                        {hit.snippet}
                    </span>
                </button>
            ))}
        </dialog>
    );
}

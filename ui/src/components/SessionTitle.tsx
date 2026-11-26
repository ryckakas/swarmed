import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Session } from "../api/types";
import { useStore } from "../store";
import { useWords } from "../useWords";
import { HighlightDot } from "./Highlight";

/** The session's title. Double-click it, or press Rename, to type a better one. */
export function SessionTitle({ session }: { session: Session }) {
    const words = useWords();
    const rename = useStore((state) => state.rename);
    const [draft, setDraft] = useState<string | null>(null);
    // A rename asked for elsewhere, by its shortcut, starts the same edit as the button here.
    const asked = useStore((state) => state.renameAsked);
    const seen = useRef(asked);
    useEffect(() => {
        if (asked !== seen.current) {
            seen.current = asked;
            setDraft(session.title);
        }
    }, [asked, session.title]);
    const title = session.title || words.untitled;

    if (draft === null) {
        return (
            <div className="session-title-row">
                <HighlightDot colour={session.highlight} />
                <h1 title="Double-click to rename" onDoubleClick={() => setDraft(session.title)}>
                    {title}
                </h1>
                <button
                    type="button"
                    className="title-rename"
                    onClick={() => setDraft(session.title)}
                >
                    Rename
                </button>
            </div>
        );
    }
    const save = () => {
        const wanted = draft.trim();
        if (wanted !== "" && wanted !== session.title) {
            void rename(api, session.id, wanted);
        }
        setDraft(null);
    };
    const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
        if (event.key === "Enter") {
            save();
        } else if (event.key === "Escape") {
            setDraft(null);
        }
    };
    return (
        <input
            className="session-title-input"
            aria-label="Session title"
            value={draft}
            maxLength={120}
            // biome-ignore lint/a11y/noAutofocus: the user just asked to type here
            autoFocus
            onFocus={(event) => event.currentTarget.select()}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={onKeyDown}
            onBlur={save}
        />
    );
}

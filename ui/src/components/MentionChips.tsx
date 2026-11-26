import { useEffect, useState } from "react";
import { api } from "../api";
import type { PickedElement } from "../api/types";
import type { ListedElement } from "../elements";
import { type GitHubMentioned, githubSaid, mentionedPieces } from "../githubMentions";
import { keptImage } from "../keptImages";
import type { Mentioned } from "../mentions";
import { useStore } from "../store";
import { findSession } from "../tree";
import { Dialog } from "./Dialog";
import { SaidText } from "./SaidText";

/**
 * A session a message mentions, by its title as it is now: pressing it opens that session.
 * One that is closed or gone keeps the title it had when the message went, and opens nothing.
 */
export function MentionChip({ session }: { session: Mentioned }) {
    const now = useStore((state) => findSession(state.projects, session.id));
    const report = useStore((state) => state.report);
    const title = (now ? now.title : session.title) || `Session ${session.id}`;
    const hint = now
        ? `${title}: open session ${session.id}`
        : `${title}: session ${session.id} is closed or gone`;
    return (
        <button
            type="button"
            className="mention-chip"
            title={hint}
            disabled={!now}
            onClick={() => void useStore.getState().open(api, session.id).catch(report)}
        >
            <span className="mention-at" aria-hidden="true">
                @
            </span>
            {title}
        </button>
    );
}

/**
 * A pull request or an issue a message mentions, coloured by its state when the message went:
 * pressing it opens it on GitHub.
 */
export function GitHubChip({ item, shown }: { item: GitHubMentioned; shown?: string }) {
    const report = useStore((state) => state.report);
    const title = shown ?? item.title;
    return (
        <button
            type="button"
            className="mention-chip github-chip"
            data-state={item.state}
            title={`${githubSaid(item)}: ${item.title}. Open it on GitHub`}
            onClick={() => void api.openLink(item.url).catch(report)}
        >
            <span className="github-number" data-state={item.state}>
                #{item.number}
            </span>
            {title && ` ${title}`}
        </button>
    );
}

/**
 * An element picked in the task's browser that a message points at, with its picture: pressing
 * it shows the picture large, with what was read of the element.
 */
export function ElementChip({ element, sessionId }: { element: ListedElement; sessionId: number }) {
    const [open, setOpen] = useState(false);
    const picture = usePicture(sessionId, element.picture);
    return (
        <>
            <button
                type="button"
                className="mention-chip element-chip"
                title={`${element.label}: show it large`}
                onClick={() => setOpen(true)}
            >
                {picture ? (
                    <img className="element-thumb" src={picture} alt="" />
                ) : (
                    <span className="mention-at" aria-hidden="true">
                        ⌖
                    </span>
                )}
                {element.label}
            </button>
            {open && (
                <ElementDialog
                    element={element}
                    sessionId={sessionId}
                    picture={picture}
                    onClose={() => setOpen(false)}
                />
            )}
        </>
    );
}

/** The picture at `path`, kept for session `sessionId`, once read; `null` until then, or if there is none. */
function usePicture(sessionId: number, path: string | null): string | null {
    const [source, setSource] = useState<string | null>(null);
    useEffect(() => {
        if (path === null) {
            return;
        }
        let current = true;
        keptImage(sessionId, path, api.sessionImage).then(
            (found) => current && setSource(found),
            () => {},
        );
        return () => {
            current = false;
        };
    }, [sessionId, path]);
    return source;
}

/**
 * An element a message points at, large: its picture, where it was and how it is found, and
 * what was read of it when it was picked, its styles and HTML, which are the page's own.
 */
function ElementDialog(props: {
    element: ListedElement;
    sessionId: number;
    picture: string | null;
    onClose(): void;
}) {
    const { element, sessionId, picture, onClose } = props;
    const report = useStore((state) => state.report);
    const kept = useKeptElement(sessionId, element.number);
    const path = element.picture;
    const open = path && (() => void api.sessionImageOpen(sessionId, path).catch(report));
    return (
        <Dialog
            title={`Element ${element.number}: ${element.label}`}
            size="large"
            onClose={onClose}
            closeOnBackdrop
            actions={
                open && (
                    <button type="button" className="button" onClick={open}>
                        Open in the viewer
                    </button>
                )
            }
        >
            {picture ? (
                <img className="element-picture" src={picture} alt={element.label} />
            ) : (
                <p className="aside">No picture was taken of it.</p>
            )}
            <dl className="facts element-said">
                <ElementField name="Address" value={element.url} mono />
                <ElementField name="Selector" value={element.selector} mono />
                <ElementField name="Role" value={element.role} />
                <ElementField name="Box" value={element.box} />
                <ElementField
                    name="Styles"
                    value={
                        kept?.styles
                            .map((style) => `${style.property}: ${style.value}`)
                            .join("; ") ?? null
                    }
                    mono
                />
            </dl>
            {kept && kept.html !== "" && (
                <details className="element-html">
                    <summary>Its HTML, as the page had it</summary>
                    <pre>{kept.html}</pre>
                </details>
            )}
        </Dialog>
    );
}

/** One line of what was read of an element; nothing where nothing was read. */
function ElementField(props: { name: string; value: string | null; mono?: boolean }) {
    if (props.value === null || props.value === "") {
        return null;
    }
    return (
        <>
            <dt>{props.name}</dt>
            <dd className={props.mono ? "mono" : undefined}>{props.value}</dd>
        </>
    );
}

/** What the core kept of element `number` of session `sessionId`, once read; `null` if it is gone. */
function useKeptElement(sessionId: number, number: number): PickedElement | null {
    const [kept, setKept] = useState<PickedElement | null>(null);
    useEffect(() => {
        let current = true;
        api.sessionElements(sessionId, [number]).then(
            ([found]) => current && setKept(found ?? null),
            () => {},
        );
        return () => {
            current = false;
        };
    }, [sessionId, number]);
    return kept;
}

interface MentionedProps {
    text: string;
    sessions: Mentioned[];
    github: GitHubMentioned[];
    elements: ListedElement[];
    /** The session the message went to, whose folder holds the elements' pictures. */
    sessionId: number;
}

/**
 * What the user wrote, with the sessions, pull requests, issues and picked elements listed
 * under the message drawn where their tokens stand, its addresses as links and its code and
 * paths as code.
 */
export function WithMentions({ text, sessions, github, elements, sessionId }: MentionedProps) {
    if (sessions.length === 0 && github.length === 0 && elements.length === 0) {
        return <SaidText text={text} sessionId={sessionId} />;
    }
    return mentionedPieces(text, sessions, github, elements).map((piece, index) => {
        if (typeof piece === "string") {
            // biome-ignore lint/suspicious/noArrayIndexKey: the pieces of a sent message never change
            return <SaidText key={index} text={piece} sessionId={sessionId} />;
        }
        if ("session" in piece) {
            // biome-ignore lint/suspicious/noArrayIndexKey: a session can be named twice
            return <MentionChip key={index} session={piece.session} />;
        }
        if ("element" in piece) {
            // biome-ignore lint/suspicious/noArrayIndexKey: an element can be named twice
            return <ElementChip key={index} element={piece.element} sessionId={sessionId} />;
        }
        // biome-ignore lint/suspicious/noArrayIndexKey: a pull request can be named twice
        return <GitHubChip key={index} item={piece.github} shown={piece.github.shown} />;
    });
}

/**
 * Sessions, pull requests, issues and picked elements a message mentions that its words name
 * nowhere, as on the command line: under it.
 */
export function MentionList({
    sessions,
    github,
    elements,
    sessionId,
}: Omit<MentionedProps, "text">) {
    if (sessions.length === 0 && github.length === 0 && elements.length === 0) {
        return null;
    }
    return (
        <ul className="mention-chips" aria-label="Mentioned">
            {sessions.map((session) => (
                <li key={`@${session.id}`}>
                    <MentionChip session={session} />
                </li>
            ))}
            {github.map((item) => (
                <li key={`#${item.number}`}>
                    <GitHubChip item={item} />
                </li>
            ))}
            {elements.map((element) => (
                <li key={`element ${element.number}`}>
                    <ElementChip element={element} sessionId={sessionId} />
                </li>
            ))}
        </ul>
    );
}

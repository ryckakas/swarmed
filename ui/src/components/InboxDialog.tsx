import { type KeyboardEvent, useEffect, useRef } from "react";
import { api } from "../api";
import { INBOX_KINDS, type InboxItem, type InboxKind, inbox } from "../inbox";
import { useStore } from "../store";
import { useWords } from "../useWords";
import { wordsFor } from "../wording";
import { StatusMark } from "./StatusMark";

/** Up and down move between the rows, as in a list. */
function moveFocus(event: KeyboardEvent<HTMLElement>) {
    const step = { ArrowDown: 1, ArrowUp: -1 }[event.key];
    if (step === undefined) {
        return;
    }
    const rows = [...event.currentTarget.querySelectorAll<HTMLButtonElement>(".inbox-row")];
    const next = rows[rows.indexOf(document.activeElement as HTMLButtonElement) + step];
    if (next) {
        event.preventDefault();
        next.focus();
    }
}

/**
 * Everything that waits for the user, in one list: sessions that ask for a decision, that
 * failed, or that hold an answer nobody has looked at. Choosing one opens it.
 */
export function InboxDialog() {
    const words = useWords();
    const open = useStore((state) => state.inboxOpen);
    const showInbox = useStore((state) => state.showInbox);
    const projects = useStore((state) => state.projects);
    const dialog = useRef<HTMLDialogElement>(null);

    // A modal <dialog> keeps the keyboard inside it and closes on Esc by itself.
    useEffect(() => {
        if (open && !dialog.current?.open) {
            dialog.current?.showModal();
        }
    }, [open]);

    if (!open) {
        return null;
    }
    const items = inbox(projects);
    return (
        <dialog
            ref={dialog}
            className="confirm inbox"
            aria-labelledby="inbox-title"
            onClose={() => showInbox(false)}
            onKeyDown={moveFocus}
            // A click on the backdrop, outside the list, puts it away.
            onClick={(event) => event.target === dialog.current && showInbox(false)}
        >
            <h2 id="inbox-title">{words.waitingForYou}</h2>
            {items.length === 0 && <p>{words.nothingWaits}</p>}
            {INBOX_KINDS.map((kind) => (
                <InboxGroup
                    key={kind}
                    kind={kind}
                    items={items.filter((item) => item.kind === kind)}
                />
            ))}
        </dialog>
    );
}

function InboxGroup({ kind, items }: { kind: InboxKind; items: InboxItem[] }) {
    const words = useWords();
    const heading = useStore((state) => wordsFor(state.settings?.style).status[kind][0]);
    const openSession = useStore((state) => state.open);
    const showInbox = useStore((state) => state.showInbox);
    if (items.length === 0) {
        return null;
    }
    const go = (id: number) => {
        showInbox(false);
        void openSession(api, id);
    };
    return (
        <section className="inbox-group" aria-label={heading}>
            <h3>{heading}</h3>
            {items.map(({ session, project, task }) => (
                <button
                    key={session.id}
                    type="button"
                    className="inbox-row"
                    onClick={() => go(session.id)}
                >
                    <StatusMark status={session.status} unread={session.unread} />
                    <span className="inbox-title">{session.title || words.untitled}</span>
                    <span className="inbox-place">
                        {project} / {task}
                    </span>
                </button>
            ))}
        </section>
    );
}

import { useEffect, useRef } from "react";
import { api } from "../api";
import { useStore } from "../store";
import { findSession } from "../tree";

/** Asks before a session is closed. Nothing is closed by one stray click. */
export function CloseSessionDialog() {
    const closing = useStore((state) => state.closing);
    const session = useStore((state) => findSession(state.projects, state.closing));
    const askToClose = useStore((state) => state.askToClose);
    const close = useStore((state) => state.close);
    const dialog = useRef<HTMLDialogElement>(null);

    // A modal <dialog> keeps the keyboard inside it and closes on Esc by itself.
    useEffect(() => {
        if (session && !dialog.current?.open) {
            dialog.current?.showModal();
        }
    }, [session]);

    if (closing === null || !session) {
        return null;
    }
    const busy = session.status === "working" || session.status === "waiting";
    return (
        <dialog
            ref={dialog}
            className="confirm"
            aria-labelledby="confirm-title"
            onClose={() => askToClose(null)}
        >
            <h2 id="confirm-title">Close this session?</h2>
            <p className="confirm-what">{session.title || "New session"}</p>
            <p>
                {busy ? "The agent is still working and will be stopped. " : ""}
                The session leaves the sidebar. Its files and the task stay as they are.
            </p>
            {/* Opening the dialog focuses its first button, so a stray Enter keeps the session. */}
            <div className="confirm-actions">
                <button type="button" className="button" onClick={() => askToClose(null)}>
                    Keep it
                </button>
                <button
                    type="button"
                    className="button button-primary"
                    onClick={() => void close(api, closing)}
                >
                    Close session
                </button>
            </div>
        </dialog>
    );
}

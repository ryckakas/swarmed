import { useEffect, useRef } from "react";
import { api } from "../api";
import { useStore } from "../store";
import { findSession } from "../tree";
import { useWords } from "../useWords";

/**
 * Asks what to do with a session whose close button was pressed: keep it, put it away with its
 * conversation kept, or delete it for good. For a session that is put away already, only the
 * last is left to ask. Nothing goes by one stray click.
 */
export function CloseSessionDialog() {
    const closing = useStore((state) => findSession(state.projects, state.closing));
    const deleting = useStore((state) => state.deleting);
    const askToClose = useStore((state) => state.askToClose);
    const askToDelete = useStore((state) => state.askToDelete);
    const close = useStore((state) => state.close);
    const remove = useStore((state) => state.deleteSession);
    const words = useWords();
    const dialog = useRef<HTMLDialogElement>(null);
    const asked = closing ?? deleting;

    // A modal <dialog> keeps the keyboard inside it and closes on Esc by itself.
    useEffect(() => {
        if (asked && !dialog.current?.open) {
            dialog.current?.showModal();
        }
    }, [asked]);

    if (!asked) {
        return null;
    }
    const withdraw = () => {
        askToClose(null);
        askToDelete(null);
    };
    const busy = closing?.status === "working" || closing?.status === "waiting";
    return (
        <dialog ref={dialog} className="confirm" aria-labelledby="confirm-title" onClose={withdraw}>
            <h2 id="confirm-title">
                {closing ? words.closeAsk : `${words.deleteSession} this session for good?`}
            </h2>
            <p className="confirm-what">{asked.title || "New session"}</p>
            {closing && (
                <p>
                    {busy ? "The agent is still working and will be stopped. " : ""}
                    <strong>{words.archiveSession}</strong> takes the session out of the sidebar and
                    keeps its conversation: it can be brought back from under its task.
                </p>
            )}
            <p>
                <strong>{words.deleteSession}</strong> removes the conversation for good, to free
                the room it takes. This cannot be undone. The task and its files stay as they are
                either way.
            </p>
            {/* Opening the dialog focuses its first button, so a stray Enter keeps the session. */}
            <div className="confirm-actions">
                <button type="button" className="button" onClick={withdraw}>
                    Keep it
                </button>
                <button
                    type="button"
                    className="button button-danger"
                    onClick={() => void remove(api, asked.id)}
                >
                    {words.deleteSession}
                </button>
                {closing && (
                    <button
                        type="button"
                        className="button button-primary"
                        onClick={() => void close(api, closing.id)}
                    >
                        {words.archiveSession}
                    </button>
                )}
            </div>
        </dialog>
    );
}

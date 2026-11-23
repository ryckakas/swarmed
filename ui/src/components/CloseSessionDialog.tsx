import { api } from "../api";
import { useStore } from "../store";
import { findSession } from "../tree";
import { useWords } from "../useWords";
import { Dialog } from "./Dialog";

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
    const asked = closing ?? deleting;

    if (!asked) {
        return null;
    }
    const withdraw = () => {
        askToClose(null);
        askToDelete(null);
    };
    const busy = closing?.status === "working" || closing?.status === "waiting";
    // Putting away is the main button, since it loses nothing. Asked only to delete, there is
    // none, and the keyboard starts on keeping the session.
    return (
        <Dialog
            title={closing ? words.closeAsk : `${words.deleteSession} this session for good?`}
            size="small"
            closeOnBackdrop={true}
            onClose={withdraw}
            dismiss="Keep it"
            actions={
                <>
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
                </>
            }
        >
            <p className="dialog-subject">{asked.title || words.untitled}</p>
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
        </Dialog>
    );
}

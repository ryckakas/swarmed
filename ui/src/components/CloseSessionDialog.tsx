import { useState } from "react";
import { api } from "../api";
import type { Session } from "../api/types";
import { alsoCloseSaid, workerRows } from "../closing";
import { useStore } from "../store";
import { findSession } from "../tree";
import { useWords } from "../useWords";
import { useAsked, WorkersToClose } from "./CloseWorkers";
import { Dialog } from "./Dialog";

/**
 * Asks what to do with a session whose close button was pressed: keep it, put it away with its
 * conversation kept, or delete it for good. For a session that is put away already, only the
 * last is left to ask. Nothing goes by one stray click.
 */
export function CloseSessionDialog() {
    const closing = useStore((state) => findSession(state.projects, state.closing));
    const deleting = useStore((state) => state.deleting);
    const asked = closing ?? deleting;
    if (!asked) {
        return null;
    }
    // Asked about another, it starts again: nothing ticked for the last carries over.
    return closing ? (
        <CloseAsk key={`close-${closing.id}`} session={closing} />
    ) : (
        <DeleteAsk key={`delete-${asked.id}`} session={asked} />
    );
}

/** Withdraws the question, whichever it was. */
function useWithdraw() {
    const askToClose = useStore((state) => state.askToClose);
    const askToDelete = useStore((state) => state.askToDelete);
    return () => {
        askToClose(null);
        askToDelete(null);
    };
}

/** The button that deletes the session for good, open or closed. */
function DeleteButton({ id }: { id: number }) {
    const words = useWords();
    const remove = useStore((state) => state.deleteSession);
    return (
        <button type="button" className="button button-danger" onClick={() => void remove(api, id)}>
            {words.deleteSession}
        </button>
    );
}

/** What deleting does, said under either question. */
function DeleteSaid() {
    const words = useWords();
    return (
        <p>
            <strong>{words.deleteSession}</strong> removes the conversation for good, to free the
            room it takes. This cannot be undone. The task and its files stay as they are either
            way.
        </p>
    );
}

/**
 * Closing an open session. When others work for it, however far down and in whichever task,
 * they are listed, and a tick box, unticked to begin with, closes them too.
 */
function CloseAsk({ session }: { session: Session }) {
    const words = useWords();
    const withdraw = useWithdraw();
    const close = useStore((state) => state.close);
    const projects = useStore((state) => state.projects);
    const under = useAsked(String(session.id), () => api.sessionDescendants(session.id, false));
    const rows = workerRows(under, projects, session.task_id);
    const [ticked, setTicked] = useState(false);
    const busy = session.status === "working" || session.status === "waiting";
    const withWorkers = ticked && rows.length > 0;
    // Putting away is the main button, since it loses nothing.
    return (
        <Dialog
            title={words.closeAsk}
            size={rows.length > 0 ? "medium" : "small"}
            closeOnBackdrop={!withWorkers}
            onClose={withdraw}
            dismiss="Keep it"
            actions={
                <>
                    <DeleteButton id={session.id} />
                    <button
                        type="button"
                        className="button button-primary"
                        onClick={() => void close(api, session.id, withWorkers)}
                    >
                        {words.archiveSession}
                    </button>
                </>
            }
        >
            <p className="dialog-subject">{session.title || words.untitled}</p>
            {rows.length > 0 && (
                <WorkersToClose
                    heading={`${words.workingFor}:`}
                    rows={rows}
                    label={alsoCloseSaid(rows.length, words)}
                    ticked={ticked}
                    disabled={false}
                    onTick={setTicked}
                />
            )}
            <p>
                {busy ? "The agent is still working and will be stopped. " : ""}
                <strong>{words.archiveSession}</strong> takes the session out of the sidebar and
                keeps its conversation: it can be brought back from under its task.
                {rows.length > 0 && !withWorkers
                    ? ` Those that work for it stay open, working for it.`
                    : ""}
            </p>
            <DeleteSaid />
        </Dialog>
    );
}

/** Deleting a session that is put away already: the only thing left to ask. */
function DeleteAsk({ session }: { session: { id: number; title: string } }) {
    const words = useWords();
    const withdraw = useWithdraw();
    // There is no main button: the keyboard starts on keeping the session.
    return (
        <Dialog
            title={`${words.deleteSession} this session for good?`}
            size="small"
            closeOnBackdrop={true}
            onClose={withdraw}
            dismiss="Keep it"
            actions={<DeleteButton id={session.id} />}
        >
            <p className="dialog-subject">{session.title || words.untitled}</p>
            <DeleteSaid />
        </Dialog>
    );
}

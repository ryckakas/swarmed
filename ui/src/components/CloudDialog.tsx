import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { NoteTarget } from "../api/types";
import { useStore } from "../store";

/** Hands a piece of work to a cloud agent, from a repository or from one of its tasks. */
export function CloudDialog() {
    const target = useStore((state) => state.delegating);
    if (target === null) {
        return null;
    }
    return <Delegate key={`${target.project}/${target.task ?? ""}`} target={target} />;
}

function Delegate({ target }: { target: NoteTarget }) {
    const close = () => useStore.getState().askToDelegate(null);
    const dialog = useRef<HTMLDialogElement>(null);
    const [prompt, setPrompt] = useState("");
    const [sending, setSending] = useState(false);
    // Why it was refused, in the core's own words: they say what to do about it.
    const [problem, setProblem] = useState<string | null>(null);

    useEffect(() => {
        dialog.current?.showModal();
    }, []);

    const send = async () => {
        if (prompt.trim() === "" || sending) {
            return;
        }
        setSending(true);
        setProblem(null);
        try {
            await api.cloudDelegate(target.project, prompt.trim(), target.task);
            await useStore.getState().refresh(api);
            close();
        } catch (error) {
            setProblem(String(error));
            setSending(false);
        }
    };
    const onKeyDown = (event: KeyboardEvent) => {
        if (event.key === "Enter" && event.metaKey) {
            event.preventDefault();
            void send();
        }
    };

    return (
        <dialog ref={dialog} className="confirm note" aria-labelledby="cloud-title" onClose={close}>
            <h2 id="cloud-title">Delegate to the cloud from {target.task ?? target.project}</h2>
            <p>
                A Codex agent works on this in the cloud, on the repository as it is on GitHub
                {target.task
                    ? ": from this task's branch if it has been pushed, else from the base branch."
                    : ", from its base branch."}{" "}
                It does not see what is only on this Mac, and it carries on when Swarmed is closed.
                You follow it at Codex.
            </p>
            <textarea
                className="note-text"
                aria-label="What the cloud agent is to do"
                rows={8}
                placeholder="Say everything it needs to know: it starts with no knowledge of your sessions here."
                disabled={sending}
                value={prompt}
                onChange={(event) => setPrompt(event.target.value)}
                onKeyDown={onKeyDown}
            />
            {problem && <p className="note-problem">{problem}</p>}
            <div className="confirm-actions">
                <button type="button" className="button" onClick={close}>
                    Cancel
                </button>
                <button
                    type="button"
                    className="button button-primary"
                    disabled={prompt.trim() === "" || sending}
                    title="⌘ Enter"
                    onClick={() => void send()}
                >
                    {sending ? "Delegating…" : "Delegate"}
                </button>
            </div>
        </dialog>
    );
}

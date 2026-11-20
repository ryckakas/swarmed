import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { CloudProvider, NoteTarget } from "../api/types";
import { CLOUD_PROVIDERS, cloudProvider } from "../cloud";
import { useStore } from "../store";

/** Hands a piece of work to a cloud agent, from a repository or from one of its tasks. */
export function CloudDialog() {
    const target = useStore((state) => state.delegating);
    if (target === null) {
        return null;
    }
    return <Delegate key={`${target.project}/${target.task ?? ""}`} target={target} />;
}

/** What is particular to each provider, said before anything is sent. */
const PROVIDER_NOTES: Record<CloudProvider, string> = {
    codex: "Runs in the Codex cloud environment set up for this repository, and is refused if there is none. From a task, it starts from the task's branch if that has been pushed, else from the base branch. Its status shows here.",
    claude: "Runs in your default Claude Code cloud environment, on the repository as its own folder has it checked out. Claude Code has to trust that folder first. Its progress does not show here, only its link.",
};

function Delegate({ target }: { target: NoteTarget }) {
    const close = () => useStore.getState().askToDelegate(null);
    const dialog = useRef<HTMLDialogElement>(null);
    const [prompt, setPrompt] = useState("");
    const [provider, setProvider] = useState<CloudProvider>("codex");
    const { name } = cloudProvider(provider);
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
            await api.cloudDelegate(target.project, prompt.trim(), target.task, provider);
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
                An agent works on this in the cloud, on the repository as it is on GitHub. It does
                not see what is only on this Mac, and it carries on when Swarmed is closed. You
                follow it at {name}.
            </p>
            <label className="cloud-providers">
                Whose cloud
                <select
                    className="picker"
                    value={provider}
                    disabled={sending}
                    onChange={(event) => {
                        const chosen = CLOUD_PROVIDERS.find(([id]) => id === event.target.value);
                        setProvider(chosen?.[0] ?? provider);
                    }}
                >
                    {CLOUD_PROVIDERS.map(([id, label]) => (
                        <option key={id} value={id}>
                            {label}
                        </option>
                    ))}
                </select>
            </label>
            <p className="settings-hint">{PROVIDER_NOTES[provider]}</p>
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

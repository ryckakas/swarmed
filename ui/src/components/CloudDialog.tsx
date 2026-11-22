import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { sendOrQueue } from "../actions";
import { api } from "../api";
import type { CloudProvider } from "../api/types";
import {
    briefRequest,
    CLOUD_PROVIDERS,
    CLOUD_TELLINGS,
    type CloudTarget,
    type CloudTelling,
    cloudProvider,
    unseenByCloud,
} from "../cloud";
import { useStore } from "../store";
import { useWords } from "../useWords";
import type { Words } from "../wording";

/** Hands a piece of work to a cloud agent: from a repository, a task, or a session of one. */
export function CloudDialog() {
    const target = useStore((state) => state.delegating);
    if (target === null) {
        return null;
    }
    const key = `${target.project}/${target.task ?? ""}/${target.session ?? ""}`;
    return <Delegate key={key} target={target} />;
}

/** What is particular to each provider, said before anything is sent. */
const PROVIDER_NOTES: Record<CloudProvider, string> = {
    codex: "Runs in the Codex cloud environment set up for this repository, and is refused if there is none. From a task, it starts from the task's branch if that has been pushed, else from the base branch. Its status shows here.",
    claude: "Runs in your default Claude Code cloud environment, on the repository as its own folder has it checked out. Claude Code has to trust that folder first. Its progress does not show here, only its link.",
};

/** What the message box asks for, by what the cloud agent is told. */
const PLACEHOLDERS: Record<CloudTelling, string> = {
    written: "Say everything it needs to know: it starts with no knowledge of your sessions here.",
    brief: "Anything to add for the agent that writes the brief. May be left empty.",
    conversation:
        "What it is to do with the conversation. Left empty, it carries on where the conversation ended.",
};

/** What of the task's work is only on this machine, once git has been asked. */
function useUnseen(target: CloudTarget): string | null {
    const [unseen, setUnseen] = useState<string | null>(null);
    const { project, task } = target;
    useEffect(() => {
        if (task === null) {
            return;
        }
        let current = true;
        api.taskUnpublished(project, task).then(
            (unpublished) => current && setUnseen(unseenByCloud(unpublished)),
            // Not knowing is no reason to keep the form from being used.
            () => undefined,
        );
        return () => {
            current = false;
        };
    }, [project, task]);
    return unseen;
}

/** Hands the work over the way the form says. Resolves once it is on its way. */
async function handOver(
    target: CloudTarget,
    telling: CloudTelling,
    provider: CloudProvider,
    text: string,
): Promise<void> {
    const { session } = target;
    if (session !== undefined && telling === "brief") {
        // The session's agent does the delegating, in the conversation, where it is seen.
        await sendOrQueue(session, briefRequest(provider, text));
        return;
    }
    if (session !== undefined && telling === "conversation") {
        await api.cloudDelegateConversation(session, text, provider);
    } else {
        await api.cloudDelegate(target.project, text, target.task, provider);
    }
    await useStore.getState().refresh(api);
}

function Delegate({ target }: { target: CloudTarget }) {
    const words = useWords();
    const close = () => useStore.getState().askToDelegate(null);
    const dialog = useRef<HTMLDialogElement>(null);
    const fromSession = target.session !== undefined;
    const [prompt, setPrompt] = useState("");
    const [provider, setProvider] = useState<CloudProvider>("codex");
    const [telling, setTelling] = useState<CloudTelling>(fromSession ? "brief" : "written");
    const { name } = cloudProvider(provider);
    const [sending, setSending] = useState(false);
    // Why it was refused, in the core's own words: they say what to do about it.
    const [problem, setProblem] = useState<string | null>(null);
    const unseen = useUnseen(target);
    const ready = !sending && (telling !== "written" || prompt.trim() !== "");

    useEffect(() => {
        dialog.current?.showModal();
    }, []);

    const send = async () => {
        if (!ready) {
            return;
        }
        setSending(true);
        setProblem(null);
        try {
            await handOver(target, telling, provider, prompt.trim());
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
            <h2 id="cloud-title">
                {words.delegate} from {target.task ?? target.project}
            </h2>
            <p>
                An agent starts on this in the cloud at once, on the repository as it is on GitHub.
                It does not see what is only on this Mac. It needs neither Swarmed nor this Mac to
                keep working, and you follow it at {name}.
            </p>
            {unseen && <p className="note-problem">{unseen}</p>}
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
            {fromSession && <Telling telling={telling} disabled={sending} onChoose={setTelling} />}
            <textarea
                className="note-text"
                aria-label={`What the cloud ${words.terms.agent} is to do`}
                rows={telling === "written" ? 8 : 4}
                placeholder={PLACEHOLDERS[telling]}
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
                    disabled={!ready}
                    title="⌘ Enter"
                    onClick={() => void send()}
                >
                    {sendLabel(telling, sending, words)}
                </button>
            </div>
        </dialog>
    );
}

/** What the button that sends says. */
function sendLabel(telling: CloudTelling, sending: boolean, words: Words): string {
    if (telling === "brief") {
        return sending ? "Asking…" : `Ask the ${words.terms.agent}`;
    }
    return sending ? words.delegating : words.delegateButton;
}

interface TellingProps {
    telling: CloudTelling;
    disabled: boolean;
    onChoose(telling: CloudTelling): void;
}

/** The choice of what the cloud agent is told, with what the chosen way means under it. */
function Telling({ telling, disabled, onChoose }: TellingProps) {
    const words = useWords();
    const note = CLOUD_TELLINGS.find(([id]) => id === telling)?.[2];
    return (
        <>
            <label className="cloud-providers">
                What it is told
                <select
                    className="picker"
                    value={telling}
                    disabled={disabled}
                    onChange={(event) => {
                        const chosen = CLOUD_TELLINGS.find(([id]) => id === event.target.value);
                        onChoose(chosen?.[0] ?? telling);
                    }}
                >
                    {CLOUD_TELLINGS.map(([id, label]) => (
                        <option key={id} value={id}>
                            {label(words)}
                        </option>
                    ))}
                </select>
            </label>
            {note && <p className="settings-hint">{note}</p>}
        </>
    );
}

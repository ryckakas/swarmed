import { type KeyboardEvent, lazy, Suspense, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Session, Supervision } from "../api/types";
import { useStore } from "../store";
import { ComposerBox } from "./ComposerBox";
import { CLAUDE, modelChoices, supervisionChoices, useAgents, useSlashCommands } from "./options";
import { Picker } from "./Picker";

// The terminal emulator is a large library; it is only fetched when a terminal is first shown.
const TerminalBox = lazy(async () => {
    const module = await import("./TerminalBox");
    return { default: module.TerminalBox };
});

const PLACEHOLDER: Record<Session["status"], string> = {
    idle: "Reply to the agent",
    working: "The agent is working. Press Esc to stop it.",
    waiting: "Answer the request above first",
    errored: "Send a message to start the agent again",
};

/** The message box of an existing session. */
export function Composer({ session }: { session: Session }) {
    const [text, setText] = useState("");
    const [terminal, setTerminal] = useState(false);
    const report = useStore((state) => state.report);
    const input = useRef<HTMLTextAreaElement>(null);
    const { id, status } = session;
    const busy = status === "working" || status === "waiting";
    const commands = useSlashCommands(id, status);
    const canSend = !busy && text.trim() !== "";

    // Answering a permission request removes the button that had focus. Bring it back here, so
    // the keyboard is never left pointing at nothing.
    // biome-ignore lint/correctness/useExhaustiveDependencies: runs when the status changes
    useEffect(() => {
        if (document.activeElement === document.body) {
            input.current?.focus();
        }
    }, [status]);

    const send = () => {
        if (canSend) {
            setText("");
            api.sessionSend(id, text.trim()).catch(report);
        }
    };
    const stop = () => void api.sessionInterrupt(id).catch(report);

    const share = () => api.sessionSend(id, "I ran this in the terminal.").catch(report);
    // Ctrl+` switches between the message box and the terminal, from either.
    const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
        if (event.ctrlKey && event.key === "`") {
            event.preventDefault();
            setTerminal((shown) => !shown);
        }
    };

    return (
        // biome-ignore lint/a11y/noStaticElementInteractions: a shortcut for what the buttons inside do
        <div className="composer" onKeyDown={onKeyDown}>
            {terminal ? (
                <div className="terminal-pane">
                    <Suspense fallback={<div className="terminal-box" />}>
                        <TerminalBox sessionId={id} />
                    </Suspense>
                    <div className="terminal-foot">
                        <p>What you run here goes to the agent with your next message.</p>
                        <button type="button" className="button" disabled={busy} onClick={share}>
                            Send to the agent now
                        </button>
                        <button
                            type="button"
                            className="button"
                            title="Ctrl+`"
                            onClick={() => setTerminal(false)}
                        >
                            Back to the message
                        </button>
                    </div>
                </div>
            ) : (
                <ComposerBox
                    value={text}
                    onChange={setText}
                    onSend={send}
                    onStop={status === "working" ? stop : undefined}
                    canSend={canSend}
                    placeholder={PLACEHOLDER[status]}
                    inputRef={input}
                    commands={commands}
                >
                    <SessionPickers session={session} />
                    <button
                        type="button"
                        className="picker picker-plain"
                        title="A shell in the task's worktree (Ctrl+`)"
                        onClick={() => setTerminal(true)}
                    >
                        Terminal
                    </button>
                </ComposerBox>
            )}
        </div>
    );
}

/** The session's supervision and model, changeable at any time, also while the agent works. */
function SessionPickers({ session }: { session: Session }) {
    const configure = useStore((state) => state.configure);
    const agent = useAgents().find((candidate) => candidate.id === session.agent);
    const { id, model, supervision } = session;
    return (
        <>
            <Picker
                label="Supervision"
                value={supervision}
                onChange={(value) =>
                    void configure(api, id, { model, supervision: value as Supervision })
                }
                options={supervisionChoices(supervision)}
            />
            {session.agent !== CLAUDE && (
                <span className="option-label">{agent?.name ?? session.agent}</span>
            )}
            <Picker
                label="Model"
                value={model ?? ""}
                onChange={(value) => void configure(api, id, { model: value || null, supervision })}
                options={modelChoices(agent, model)}
            />
        </>
    );
}

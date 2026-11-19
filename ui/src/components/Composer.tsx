import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Effort, Session } from "../api/types";
import { useStore } from "../store";
import { ComposerBox } from "./ComposerBox";
import { MessageQueue } from "./MessageQueue";
import { ModePicker, TerminalIcon } from "./ModePicker";
import { CLAUDE, EFFORT_CHOICES, modelChoices, useAgents, useSlashCommands } from "./options";
import { Picker } from "./Picker";

// The terminal emulator is a large library; it is only fetched when a terminal is first shown.
const TerminalBox = lazy(async () => {
    const module = await import("./TerminalBox");
    return { default: module.TerminalBox };
});

const PLACEHOLDER: Record<Session["status"], string> = {
    idle: "Reply to the agent",
    working: "Write the next message: it waits until the agent is done. Esc stops the agent.",
    waiting: "Answer the request above. A message written here waits until the agent is done.",
    errored: "Send a message to start the agent again",
};

/** The message box of an existing session. */
export function Composer({ session }: { session: Session }) {
    const [text, setText] = useState("");
    const terminal = useStore((state) => state.terminalOpen);
    const toggleTerminal = useStore((state) => state.toggleTerminal);
    const report = useStore((state) => state.report);
    const input = useRef<HTMLTextAreaElement>(null);
    const { id, status } = session;
    const busy = status === "working" || status === "waiting";
    const commands = useSlashCommands(id, status);
    const canSend = text.trim() !== "";

    // Answering a permission request removes the button that had focus. Bring it back here, so
    // the keyboard is never left pointing at nothing.
    // biome-ignore lint/correctness/useExhaustiveDependencies: runs when the status changes
    useEffect(() => {
        if (document.activeElement === document.body) {
            input.current?.focus();
        }
    }, [status]);

    const send = () => {
        if (!canSend) {
            return;
        }
        setText("");
        // A busy agent is not interrupted: the message waits its turn, and can still be changed.
        const sent = busy ? api.sessionEnqueue(id, text.trim()) : api.sessionSend(id, text.trim());
        sent.catch(report);
    };
    const stop = () => void api.sessionInterrupt(id).catch(report);

    const share = () => api.sessionSend(id, "I ran this in the terminal.").catch(report);
    return (
        <div className="composer">
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
                            onClick={toggleTerminal}
                        >
                            Back to the message
                        </button>
                    </div>
                </div>
            ) : (
                <>
                    <MessageQueue sessionId={id} busy={busy} />
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
                            className="icon-picker"
                            aria-label="Terminal"
                            title="Terminal: a shell in the task's worktree (Ctrl+`)"
                            onClick={toggleTerminal}
                        >
                            <TerminalIcon />
                        </button>
                    </ComposerBox>
                </>
            )}
        </div>
    );
}

/** The session's supervision, model and effort, changeable at any time, also while the agent works. */
function SessionPickers({ session }: { session: Session }) {
    const configure = useStore((state) => state.configure);
    const agent = useAgents().find((candidate) => candidate.id === session.agent);
    const { id, model, supervision, effort } = session;
    const current = { model, supervision, effort };
    return (
        <>
            <ModePicker
                value={supervision}
                agentId={session.agent}
                onChange={(value) => void configure(api, id, { ...current, supervision: value })}
            />
            {session.agent !== CLAUDE && (
                <span className="option-label">{agent?.name ?? session.agent}</span>
            )}
            <Picker
                label="Model"
                value={model ?? ""}
                onChange={(value) => void configure(api, id, { ...current, model: value || null })}
                options={modelChoices(agent, model)}
            />
            {/* Only Claude Code takes an effort. Changing it restarts the agent between turns. */}
            {session.agent === CLAUDE && (
                <Picker
                    label="Effort"
                    value={effort ?? ""}
                    onChange={(value) =>
                        void configure(api, id, {
                            ...current,
                            effort: (value || null) as Effort | null,
                        })
                    }
                    options={EFFORT_CHOICES}
                />
            )}
        </>
    );
}

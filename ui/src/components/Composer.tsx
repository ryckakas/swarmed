import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Session } from "../api/types";
import { useStore } from "../store";
import { ComposerBox } from "./ComposerBox";
import { MODEL_LABELS, SUPERVISION_LABELS } from "./options";

const PLACEHOLDER: Record<Session["status"], string> = {
    idle: "Reply to the agent",
    working: "The agent is working. Press Esc to stop it.",
    waiting: "Answer the request above first",
    errored: "Send a message to start the agent again",
};

/** The message box of an existing session. */
export function Composer({ session }: { session: Session }) {
    const [text, setText] = useState("");
    const report = useStore((state) => state.report);
    const input = useRef<HTMLTextAreaElement>(null);
    const { id, status } = session;
    const busy = status === "working" || status === "waiting";
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

    return (
        <div className="composer">
            <ComposerBox
                value={text}
                onChange={setText}
                onSend={send}
                onStop={status === "working" ? stop : undefined}
                canSend={canSend}
                placeholder={PLACEHOLDER[status]}
                inputRef={input}
            >
                <span className="option-label">{SUPERVISION_LABELS[session.supervision]}</span>
                <span className="option-label">
                    {MODEL_LABELS[session.model ?? ""] ?? session.model}
                </span>
            </ComposerBox>
        </div>
    );
}

import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { SessionStatus } from "../api/types";
import { useStore } from "../store";

const PLACEHOLDER: Record<SessionStatus, string> = {
    idle: "Message the agent",
    working: "The agent is working. Press Esc to stop it.",
    waiting: "Answer the request above first",
    errored: "Send a message to start the agent again",
};

export function Composer({ sessionId, status }: { sessionId: number; status: SessionStatus }) {
    const [text, setText] = useState("");
    const report = useStore((state) => state.report);
    const input = useRef<HTMLTextAreaElement>(null);
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
            api.sessionSend(sessionId, text.trim()).catch(report);
        }
    };
    const stop = () => api.sessionInterrupt(sessionId).catch(report);

    const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
        // Enter sends; Shift+Enter is a new line. Enter that confirms an IME candidate is neither.
        if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
            event.preventDefault();
            send();
        } else if (event.key === "Escape" && status === "working") {
            void stop();
        }
    };

    return (
        <form
            className="composer"
            onSubmit={(event) => {
                event.preventDefault();
                send();
            }}
        >
            <textarea
                ref={input}
                aria-label="Message"
                rows={3}
                value={text}
                placeholder={PLACEHOLDER[status]}
                onChange={(event) => setText(event.target.value)}
                onKeyDown={onKeyDown}
            />
            <div className="composer-actions">
                {status === "working" ? (
                    <button type="button" className="button" onClick={stop}>
                        Stop
                    </button>
                ) : (
                    <button type="submit" className="button button-primary" disabled={!canSend}>
                        Send
                    </button>
                )}
            </div>
        </form>
    );
}

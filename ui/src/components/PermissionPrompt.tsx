import { useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import { summarize } from "../transcript";

interface Props {
    sessionId: number;
    requestId: string;
    toolName: string;
    input: unknown;
    description: string | null;
}

/** The agent asking to do something. It is the loudest thing on screen until it is answered. */
export function PermissionPrompt(props: Props) {
    const report = useStore((state) => state.report);
    const { sessionId, requestId, toolName, input, description } = props;
    // The prompt leaves the transcript when the app confirms the answer. Until then the buttons
    // show the click was taken, and cannot be pressed twice.
    const [sent, setSent] = useState<"allow" | "deny" | null>(null);
    const answer = (choice: "allow" | "deny") => {
        setSent(choice);
        const call =
            choice === "allow"
                ? api.sessionAllow(sessionId, requestId)
                : api.sessionDeny(sessionId, requestId, "The user declined this action.");
        call.catch((error: unknown) => {
            setSent(null);
            report(error);
        });
    };
    return (
        <section className="permission" aria-label="Permission request" aria-busy={sent !== null}>
            <p className="permission-ask">Allow {toolName}?</p>
            <pre className="permission-what">{summarize(input)}</pre>
            {description && <p className="permission-why">{description}</p>}
            <div className="permission-actions">
                {/* Focused as it appears, so Enter allows and Tab then Enter denies. */}
                <button
                    type="button"
                    className="button button-primary"
                    onClick={() => answer("allow")}
                    disabled={sent !== null}
                    // biome-ignore lint/a11y/noAutofocus: a pending request is what the user must act on next
                    autoFocus
                >
                    {sent === "allow" ? "Allowing…" : "Allow"}
                </button>
                <button
                    type="button"
                    className="button"
                    onClick={() => answer("deny")}
                    disabled={sent !== null}
                >
                    {sent === "deny" ? "Denying…" : "Deny"}
                </button>
            </div>
        </section>
    );
}

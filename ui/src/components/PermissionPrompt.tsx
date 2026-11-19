import { useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import { PLAN_TOOL, summarize } from "../transcript";
import { afterPlan, SUPERVISION_LABELS } from "./options";

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
    const words = useWords(toolName);
    const answer = (choice: "allow" | "deny") => {
        setSent(choice);
        const call =
            choice === "allow"
                ? api.sessionAllow(sessionId, requestId)
                : api.sessionDeny(sessionId, requestId, words.refusal);
        call.catch((error: unknown) => {
            setSent(null);
            report(error);
        });
    };
    return (
        <section className="permission" aria-label="Permission request" aria-busy={sent !== null}>
            <p className="permission-ask">{words.ask}</p>
            {/* A plan is shown in full just above, as the agent's own words. */}
            {words.note ? (
                <p className="permission-why">{words.note}</p>
            ) : (
                <pre className="permission-what">{summarize(input)}</pre>
            )}
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
                    {sent === "allow" ? words.allowing : words.allow}
                </button>
                <button
                    type="button"
                    className="button"
                    onClick={() => answer("deny")}
                    disabled={sent !== null}
                >
                    {sent === "deny" ? words.denying : words.deny}
                </button>
            </div>
        </section>
    );
}

/** How the question is put: a plan is approved, anything else is allowed. */
function useWords(toolName: string) {
    const next = useStore((state) => afterPlan(state.settings?.default_supervision));
    if (toolName !== PLAN_TOOL) {
        return {
            ask: `Allow ${toolName}?`,
            note: null,
            allow: "Allow",
            allowing: "Allowing…",
            deny: "Deny",
            denying: "Denying…",
            refusal: "The user declined this action.",
        };
    }
    return {
        ask: "Approve this plan?",
        note: `Once approved, the agent starts on it and plan mode ends: ${SUPERVISION_LABELS[next].toLowerCase()}.`,
        allow: "Approve plan",
        allowing: "Approving…",
        deny: "Keep planning",
        denying: "Sending back…",
        refusal: "The user does not approve this plan yet and will say what to change.",
    };
}

import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Supervision } from "../api/types";
import { focusIsFree } from "../focus";
import { questionsOf } from "../questions";
import { planKeptSaid } from "../roles";
import { useStore } from "../store";
import { PLAN_TOOL, summarize } from "../transcript";
import { findSession } from "../tree";
import { useWords } from "../useWords";
import { afterPlan, SUPERVISION_LABELS, useRoles } from "./options";
import { QuestionPrompt } from "./QuestionPrompt";

interface Props {
    sessionId: number;
    requestId: string;
    toolName: string;
    input: unknown;
    description: string | null;
}

/**
 * The agent asking for a decision: questions it puts to the user are answered where they
 * stand, anything else is allowed or denied.
 */
export function PermissionPrompt(props: Props) {
    const questions = questionsOf(props.toolName, props.input);
    if (questions) {
        const { sessionId, requestId } = props;
        return <QuestionPrompt sessionId={sessionId} requestId={requestId} questions={questions} />;
    }
    return <ToolPrompt {...props} />;
}

/** The agent asking to do something. It is the loudest thing on screen until it is answered. */
function ToolPrompt(props: Props) {
    const report = useStore((state) => state.report);
    const { sessionId, requestId, toolName, input, description } = props;
    // The prompt leaves the transcript when the app confirms the answer. Until then the buttons
    // show the click was taken, and cannot be pressed twice.
    const [sent, setSent] = useState<"allow" | "deny" | null>(null);
    const words = usePromptWords(toolName, sessionId);
    const allow = useRef<HTMLButtonElement>(null);
    // Focused as it appears, so Enter allows and Tab then Enter denies: but only while nothing
    // else has the keyboard. Taken from the message box, the next key typed would answer.
    useEffect(() => {
        if (focusIsFree(document.activeElement, document.body)) {
            allow.current?.focus();
        }
    }, []);
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
                <button
                    ref={allow}
                    type="button"
                    className="button button-primary"
                    onClick={() => answer("allow")}
                    disabled={sent !== null}
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

/**
 * How the question is put: a plan is approved, anything else is allowed. A plan of a session
 * whose role changes no files is handed on once approved, and the prompt says so.
 */
function usePromptWords(toolName: string, sessionId: number) {
    const next = useStore((state) => afterPlan(state.settings?.default_supervision));
    const role = useStore((state) => findSession(state.projects, sessionId)?.role ?? null);
    const label = (supervision: Supervision) => SUPERVISION_LABELS[supervision];
    const kept = planKeptSaid(role, useRoles(), useWords(), label);
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
        note:
            kept ??
            `Once approved, the agent starts on it and plan mode ends: ${SUPERVISION_LABELS[next].toLowerCase()}.`,
        allow: "Approve plan",
        allowing: "Approving…",
        deny: "Keep planning",
        denying: "Sending back…",
        refusal: "The user does not approve this plan yet and will say what to change.",
    };
}

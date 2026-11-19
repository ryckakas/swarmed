import { api } from "../api";
import { useStore } from "../store";
import { summarize } from "../transcript";

interface Props {
    sessionId: number;
    requestId: string;
    toolName: string;
    input: unknown;
    description: string | null;
    pending: boolean;
}

/** The agent asking to do something. While pending it is the loudest thing on screen. */
export function PermissionPrompt(props: Props) {
    const report = useStore((state) => state.report);
    const { sessionId, requestId, toolName, input, description, pending } = props;
    if (!pending) {
        return (
            <p className="asked">
                Asked to run {toolName}: <span className="mono">{summarize(input)}</span>
            </p>
        );
    }
    const allow = () => api.sessionAllow(sessionId, requestId).catch(report);
    const deny = () =>
        api.sessionDeny(sessionId, requestId, "The user declined this action.").catch(report);
    return (
        <section className="permission" aria-label="Permission request">
            <p className="permission-ask">Allow {toolName}?</p>
            <pre className="permission-what">{summarize(input)}</pre>
            {description && <p className="permission-why">{description}</p>}
            <div className="permission-actions">
                {/* Focused as it appears, so Enter allows and Tab then Enter denies. */}
                {/* biome-ignore lint/a11y/noAutofocus: a pending request is what the user must act on next */}
                <button type="button" className="button button-primary" onClick={allow} autoFocus>
                    Allow
                </button>
                <button type="button" className="button" onClick={deny}>
                    Deny
                </button>
            </div>
        </section>
    );
}

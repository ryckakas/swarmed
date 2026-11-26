import type { SentContext } from "../mcpApps";
import { useStore } from "../store";

/**
 * What apps shown in the conversation left for the agent, waiting for the next message: one
 * chip per app, its text on hover, each with a × that takes it off.
 */
export function AppContextChips({ sessionId }: { sessionId: number }) {
    const contexts = useStore((state) => state.appContexts[sessionId]);
    const keep = useStore((state) => state.keepAppContext);
    if (contexts === undefined || contexts.length === 0) {
        return null;
    }
    return (
        <ul className="file-chips app-contexts" aria-label="Left by apps for the agent">
            {contexts.map((context) => (
                <li key={context.toolUseId} className="file-chip" title={context.text}>
                    <span>For the agent, from the {context.server} app</span>
                    <button
                        type="button"
                        className="chip-remove"
                        aria-label={`Do not send what the ${context.server} app left`}
                        title="Do not send it"
                        onMouseDown={(event) => event.preventDefault()}
                        onClick={() => keep(sessionId, context.toolUseId, null)}
                    >
                        ×
                    </button>
                </li>
            ))}
        </ul>
    );
}

/** What apps left for the agent with a message, folded under it. */
export function SentContexts({ contexts }: { contexts: SentContext[] | undefined }) {
    if (contexts === undefined || contexts.length === 0) {
        return null;
    }
    return (
        <>
            {contexts.map((context, at) => (
                // Nothing else tells them apart, and they never move.
                // biome-ignore lint/suspicious/noArrayIndexKey: their order is the message's
                <details key={at} className="pasted">
                    <summary>For the agent, from the {context.server} app</summary>
                    <pre>{context.text}</pre>
                </details>
            ))}
        </>
    );
}

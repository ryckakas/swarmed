import { useEffect, useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import { useWords } from "../useWords";

/**
 * Said on the new-task screen when the agent about to be used is not on this Mac: what is
 * missing, and the way to where it can be installed. Nothing is shown while that is unknown.
 */
export function AgentNotice({ agentId }: { agentId: string }) {
    const words = useWords();
    const version = useStore((state) => state.agentsVersion);
    const showSettings = useStore((state) => state.showSettings);
    const [missing, setMissing] = useState<string | null>(null);
    // biome-ignore lint/correctness/useExhaustiveDependencies: asked again whenever agents change
    useEffect(() => {
        let current = true;
        api.agentSetups()
            .then((setups) => {
                const setup = setups.find((candidate) => candidate.id === agentId);
                if (current) {
                    setMissing(setup && setup.installed === null ? setup.name : null);
                }
            })
            // Not knowing is no reason to say anything.
            .catch(() => undefined);
        return () => {
            current = false;
        };
    }, [agentId, version]);
    if (missing === null) {
        return null;
    }
    return (
        <p className="agent-notice" role="status">
            {missing} is not installed on this Mac.
            <button type="button" className="button" onClick={() => showSettings(true, "agents")}>
                {words.installInSettings}
            </button>
        </p>
    );
}

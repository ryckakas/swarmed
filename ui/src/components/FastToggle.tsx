import { useWords } from "../useWords";

interface Props {
    on: boolean;
    onChange(on: boolean): void;
}

/**
 * Asks the agent for its fast mode: quicker answers at a higher price. Taken when the agent
 * starts, so on a running session it applies once the agent has restarted between turns.
 */
export function FastToggle({ on, onChange }: Props) {
    const words = useWords();
    return (
        <button
            type="button"
            className="fast-toggle"
            aria-pressed={on}
            title="Fast mode: quicker answers at a higher price. Applies from the agent's next start."
            onClick={() => onChange(!on)}
        >
            {words.fast}
        </button>
    );
}

import type { Effort } from "../api/types";
import { EFFORT_CHOICES } from "./options";

/** The efforts from least to most: an effort's place here is how many bars it fills. */
const LEVELS: Effort[] = ["low", "medium", "high", "xhigh", "max"];

/** How many of the five bars an effort fills; none for one that is not known. */
export function effortBars(effort: string | null | undefined): number {
    return LEVELS.indexOf(effort as Effort) + 1;
}

interface Props {
    /** The effort chosen for the session; `null` leaves it to the agent's default. */
    value: Effort | null;
    onChange(value: Effort | null): void;
    /** What the agent reports its effort comes to, when it has said: what a default means. */
    applied?: string | null;
}

/**
 * How hard the model thinks, as rising bars like a signal meter: the more of them are filled,
 * the more effort. With the agent's default, the bars show what that default comes to once the
 * agent has said, drawn fainter since it was not chosen. Underneath it is the same native menu
 * as the other pickers, and its name is the tooltip.
 */
export function EffortPicker({ value, onChange, applied = null }: Props) {
    const shown = value ?? applied;
    const filled = effortBars(shown);
    const chosen = EFFORT_CHOICES.find(([effort]) => effort === (value ?? ""))?.[1] ?? "Effort";
    const label = value === null && shown ? `${chosen}: ${shown}` : chosen;
    return (
        <span className="icon-picker effort-picker" data-chosen={value !== null} title={label}>
            <svg viewBox="0 0 16 16" aria-hidden="true">
                {LEVELS.map((level, index) => (
                    <rect
                        key={level}
                        className={index < filled ? "effort-bar effort-bar-on" : "effort-bar"}
                        x={1.5 + index * 2.8}
                        y={11 - index * 2.2}
                        width={1.8}
                        height={3 + index * 2.2}
                        rx={0.6}
                    />
                ))}
            </svg>
            <select
                aria-label={`Effort: ${label}`}
                value={value ?? ""}
                onChange={(event) => onChange((event.target.value || null) as Effort | null)}
            >
                {EFFORT_CHOICES.map(([effort, name]) => (
                    <option key={effort} value={effort}>
                        {name}
                    </option>
                ))}
            </select>
        </span>
    );
}

import type { ReactNode } from "react";
import type { Supervision } from "../api/types";
import { SUPERVISION_LABELS, supervisionChoices } from "./options";

interface Props {
    value: Supervision;
    onChange(value: Supervision): void;
    /** The agent the session runs on: plan mode is offered for Claude Code only. */
    agentId: string;
}

/**
 * The session's supervision as an icon. Underneath it is the same native menu as the other
 * pickers, so it opens and is operated like them; only what it shows while closed is a picture.
 * Its name is the tooltip.
 */
export function ModePicker({ value, onChange, agentId }: Props) {
    const label = SUPERVISION_LABELS[value];
    return (
        <span className="icon-picker" data-mode={value} title={label}>
            <svg viewBox="0 0 16 16" aria-hidden="true">
                {ICONS[value]}
            </svg>
            <select
                aria-label={`Supervision: ${label}`}
                value={value}
                onChange={(event) => onChange(event.target.value as Supervision)}
            >
                {supervisionChoices(value, agentId).map(([choice, name]) => (
                    <option key={choice} value={choice}>
                        {name}
                    </option>
                ))}
            </select>
        </span>
    );
}

/** A robot's head: the agent decides for itself. */
const AUTO = (
    <>
        <path d="M8 1.8v2" />
        <circle cx="8" cy="1.5" r="0.6" />
        <rect x="3" y="4" width="10" height="8.5" rx="2" />
        <path d="M6 7.4v1.2M10 7.4v1.2M6.5 10.5h3M1.5 7.5v2.5M14.5 7.5v2.5" />
    </>
);

/** A board on its stand with a sketch on it: the agent plans first. */
const PLAN = (
    <>
        <rect x="2" y="2" width="12" height="8.5" rx="1" />
        <path d="M4.5 8l2.2-2.6 1.8 1.6 2.8-3M8 10.5v1.5M5 14.2l3-2.2 3 2.2" />
    </>
);

/** A warning triangle: the agent asks before it runs commands. */
const ASK = (
    <>
        <path d="M8 2.2 14.3 13H1.7z" />
        <path d="M8 6.5v3M8 11.3v.2" />
    </>
);

/** A lightning bolt: nothing asks. */
const BYPASS = <path d="M9 1.5 3.5 9H8l-1 5.5L12.5 7H8z" />;

const ICONS: Record<Supervision, ReactNode> = {
    auto: AUTO,
    plan: PLAN,
    ask: ASK,
    bypass: BYPASS,
};

/** A terminal window with a prompt in it. */
export function TerminalIcon() {
    return (
        <svg viewBox="0 0 16 16" aria-hidden="true">
            <rect x="1.5" y="2.5" width="13" height="11" rx="2" />
            <path d="M4.5 6.5 6.8 8.5 4.5 10.5M8.5 10.5h3" />
        </svg>
    );
}

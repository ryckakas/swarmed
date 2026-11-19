import { useEffect, useState } from "react";
import { api } from "../api";
import type { AgentInfo, Effort, Supervision, Theme } from "../api/types";
import { useStore } from "../store";

// The choices a session is started with, and how they are named in the UI.

export const SUPERVISION_LABELS: Record<Supervision, string> = {
    ask: "Ask before commands",
    auto: "Auto mode",
    bypass: "Never ask",
};

/** What a new session can be given. Never asking is deliberately not offered here. */
export const SUPERVISION_CHOICES: Supervision[] = ["ask", "auto"];

/** Model names Claude Code accepts, by label. The empty name is its own default. */
export const MODEL_LABELS: Record<string, string> = {
    "": "Default model",
    opus: "Opus",
    sonnet: "Sonnet",
    haiku: "Haiku",
    fable: "Fable",
};

/** The id of the agent every installation has. */
export const CLAUDE = "claude";

/** A model name as shown: Claude Code's short names get labels, other names are shown as they are. */
export function modelLabel(model: string | null): string {
    return MODEL_LABELS[model ?? ""] ?? model ?? "";
}

/** The agent's default, then the models it offers, as menu options. */
export function modelChoices(agent: AgentInfo | undefined, current?: string | null) {
    const models = new Set(agent?.models ?? []);
    // A session may run on a model the list does not know; it still has to be shown as chosen.
    if (current) {
        models.add(current);
    }
    return [
        ["", modelLabel(null)],
        ...[...models].map((model): [string, string] => [model, modelLabel(model)]),
    ] satisfies [string, string][];
}

/** The supervision choices as menu options, with the current one even if it is not offered. */
export function supervisionChoices(current?: Supervision) {
    const offered = new Set<Supervision>(SUPERVISION_CHOICES);
    if (current) {
        offered.add(current);
    }
    return [...offered].map((choice): [string, string] => [choice, SUPERVISION_LABELS[choice]]);
}

/**
 * The `/` commands a message can call, for a session or, with `null`, for a new one. Read again
 * when `refreshOn` changes: an agent only reports its commands once it has started.
 */
export function useSlashCommands(sessionId: number | null, refreshOn?: unknown): string[] {
    const [commands, setCommands] = useState<string[]>([]);
    // biome-ignore lint/correctness/useExhaustiveDependencies: refreshOn exists to trigger a re-read
    useEffect(() => {
        let current = true;
        api.slashCommands(sessionId)
            .then((read) => current && setCommands(read))
            // Suggestions are a convenience: without them the box still sends what is typed.
            .catch(() => undefined);
        return () => {
            current = false;
        };
    }, [sessionId, refreshOn]);
    return commands;
}

/** The agents sessions can run on; empty until they are read. */
export function useAgents(): AgentInfo[] {
    const [agents, setAgents] = useState<AgentInfo[]>([]);
    useEffect(() => {
        api.agents().then(setAgents).catch(useStore.getState().report);
    }, []);
    return agents;
}

/** Editors offered by name, by the command that opens a folder in them. */
export const EDITORS: [command: string, name: string][] = [
    ["code", "VS Code"],
    ["cursor", "Cursor"],
    ["zed", "Zed"],
    ["subl", "Sublime Text"],
    ["idea", "IntelliJ IDEA"],
];

/** An editor command as shown: a known editor by its name, any other command as it is. */
export function editorName(command: string): string {
    return EDITORS.find(([known]) => known === command)?.[1] ?? command;
}

/** The editors as menu options, with the configured command even if it is none of them. */
export function editorChoices(current: string): [string, string][] {
    const known = EDITORS.some(([command]) => command === current);
    return known ? EDITORS : [...EDITORS, [current, current]];
}

export const THEMES: [Theme, string][] = [
    ["system", "Same as the system"],
    ["dark", "Dark"],
    ["light", "Light"],
];

/** Text sizes offered, in percent of the normal size. */
export const TEXT_SCALES: [string, string][] = [
    ["90", "Small"],
    ["100", "Normal"],
    ["110", "Large"],
    ["125", "Larger"],
    ["140", "Largest"],
];

/** The efforts a model can be asked for, as menu options. The empty one is the agent's default. */
export const EFFORT_CHOICES: [value: Effort | "", label: string][] = [
    ["", "Default effort"],
    ["low", "Low effort"],
    ["medium", "Medium effort"],
    ["high", "High effort"],
    ["xhigh", "Extra high effort"],
    ["max", "Maximum effort"],
];

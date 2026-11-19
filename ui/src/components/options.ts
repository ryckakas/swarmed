import { useEffect, useState } from "react";
import { api } from "../api";
import type { AgentInfo, Supervision } from "../api/types";
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

/** The agents sessions can run on; empty until they are read. */
export function useAgents(): AgentInfo[] {
    const [agents, setAgents] = useState<AgentInfo[]>([]);
    useEffect(() => {
        api.agents().then(setAgents).catch(useStore.getState().report);
    }, []);
    return agents;
}

import type { Supervision } from "../api/types";

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

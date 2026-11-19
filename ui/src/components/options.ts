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

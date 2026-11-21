import type { AgentEvent, AskedQuestion, ScreenSection, SlashCommand } from "./types";

// What the sample agent has for its own commands, for looking at the UI in a plain browser.

const sent = (name: string): SlashCommand => ({ name, kind: "prompt", note: "" });
const shown = (name: string): SlashCommand => ({ name, kind: "screen", note: "" });

/** The commands the sample agent offers, each with what Swarmed does with it. */
export const MOCK_COMMANDS: SlashCommand[] = [
    sent("code-review"),
    shown("context"),
    sent("frontend-design"),
    sent("grill-me"),
    sent("init"),
    sent("planning"),
    sent("review"),
    sent("security-review"),
    sent("simplify"),
    sent("tdd"),
    shown("usage"),
    shown("status"),
    shown("mcp"),
    { name: "model", kind: "setting", note: "" },
    { name: "effort", kind: "setting", note: "" },
    { name: "config", kind: "setting", note: "" },
    { name: "rename", kind: "terminal", note: "A session is renamed by pressing its title." },
];

/** Commands the sample agent only has a screen for in its own terminal, as Claude Code does. */
const SCREEN_ONLY = ["theme", "vim", "privacy-settings"];

/** The command `text` calls if it is one the sample agent can only show in its terminal. */
export function mockScreenOnly(text: string): string | null {
    const name = /^\s*\/(\S+)/.exec(text)?.[1];
    return name !== undefined && SCREEN_ONLY.includes(name) ? name : null;
}

const IN_HOURS = (hours: number) => new Date(Date.now() + hours * 3_600_000).toISOString();

const SCREENS: Record<string, () => { title: string; note: string; sections: ScreenSection[] }> = {
    usage: () => ({
        title: "Usage",
        note: "On the team plan.",
        sections: [
            {
                title: "Plan limits",
                rows: [
                    {
                        label: "Current session",
                        value: "40% used",
                        share: 40,
                        resets_at: IN_HOURS(3),
                    },
                    {
                        label: "Current week, all models",
                        value: "31% used",
                        share: 31,
                        resets_at: IN_HOURS(140),
                    },
                    {
                        label: "Current week, Opus",
                        value: "92% used",
                        share: 92,
                        resets_at: IN_HOURS(140),
                    },
                ],
            },
            {
                title: "Extra usage",
                rows: [{ label: "This month", value: "38.71 USD of 400.00 USD", share: 10 }],
            },
            {
                title: "This session",
                rows: [
                    { label: "Cost", value: "$1.24" },
                    { label: "Waiting for the model", value: "4m 12s" },
                    { label: "Lines changed", value: "128 added, 31 removed" },
                ],
            },
        ],
    }),
    status: () => ({
        title: "Status",
        note: "",
        sections: [
            {
                title: "Session",
                rows: [
                    { label: "Version", value: "2.1.330" },
                    { label: "Working folder", value: "~/swarmed/acme-web/add-invoices" },
                ],
            },
            {
                title: "Environment",
                rows: [
                    { label: "Model", value: "claude-opus-5-5" },
                    { label: "Setting sources", value: "User settings" },
                ],
            },
        ],
    }),
    context: () => ({
        title: "Context",
        note: "",
        sections: [
            { title: "", rows: [{ label: "In use", value: "250.0k of 1.0m tokens", share: 25 }] },
            {
                title: "What fills it",
                rows: [
                    { label: "Messages", value: "201.3k tokens", share: 20 },
                    { label: "Skills", value: "9.9k tokens", share: 1 },
                    { label: "System prompt", value: "3.9k tokens", share: 0 },
                ],
            },
        ],
    }),
    mcp: () => ({
        title: "MCP servers",
        note: "Reconnect, enable or disable one with /mcp reconnect, /mcp enable or /mcp disable and its name.",
        sections: [
            {
                title: "2 servers",
                rows: [
                    { label: "docs", value: "connected", note: "http, user" },
                    { label: "tracker", value: "needs auth", note: "http, project" },
                ],
            },
        ],
    }),
};

/** The sample screen for `command`, if the sample agent has one. */
export function mockScreen(command: string): AgentEvent | null {
    const screen = SCREENS[command]?.();
    return screen ? { type: "screen", command, ...screen } : null;
}

const choice = (label: string, description = "") => ({ label, description });

const one = (question: string, header: string, labels: string[]): AskedQuestion => ({
    question,
    header,
    options: labels.map((label) => choice(label)),
    multiSelect: false,
});

const QUESTIONS: Record<string, AskedQuestion[]> = {
    model: [
        one("Which model should this session run on?", "Model", [
            "Default",
            "opus",
            "sonnet",
            "haiku",
        ]),
    ],
    effort: [
        one("How hard should the model think in this session?", "Effort", [
            "Default",
            "low",
            "medium",
            "high",
            "xhigh",
            "max",
        ]),
    ],
    config: [
        one(
            "Should Claude Code compact a conversation by itself when its context fills up?",
            "Auto-compact",
            ["Leave as it is", "On", "Off"],
        ),
        one("How should Claude Code write its answers?", "Output style", [
            "Leave as it is",
            "Default",
            "Concise",
            "Explanatory",
        ]),
    ],
};

/** What the sample agent's command `command` asks, if it is one that asks. */
export function mockQuestions(command: string): AskedQuestion[] | null {
    return QUESTIONS[command] ?? null;
}

/** What came of the answers to a sample command's questions, in a line. */
export function mockSaid(command: string, answers: Record<string, string> | null): string {
    const chosen = Object.values(answers ?? {}).filter((answer) => answer !== "Leave as it is");
    if (chosen.length === 0) {
        return "Nothing was changed.";
    }
    return command === "config"
        ? `Claude Code was told: ${chosen.join(", ")}`
        : `This session's ${command} is now ${chosen[0]}.`;
}

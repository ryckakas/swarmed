// Skills as the settings show them. Pure, so it can be tested.

import type { Skill } from "./api/types";

/** Whose skills are shown, in order, with what each is called and the agent whose mark it gets. */
export const SKILL_ORIGINS: { origin: string; name: string; agent: string; about: string }[] = [
    {
        origin: "claude",
        name: "Claude Code",
        agent: "claude",
        about: "From ~/.claude/skills. Claude Code has these by itself; other agents are offered the ticked ones.",
    },
    {
        origin: "codex",
        name: "Codex",
        agent: "codex",
        about: "From ~/.codex/skills. Codex has these by itself; other agents are offered the ticked ones.",
    },
    {
        origin: "opencode",
        name: "OpenCode",
        agent: "opencode",
        about: "From OpenCode's own configuration folder.",
    },
    {
        origin: "agents",
        name: "Shared folder",
        agent: "other",
        about: "From ~/.agents/skills, which Codex and OpenCode read by themselves. Claude Code is offered the ticked ones.",
    },
];

/** The skills of one origin. */
export function skillsOf(skills: Skill[], origin: string): Skill[] {
    return skills.filter((skill) => skill.origin === origin);
}

/** How many skills there are, and how many of them are kept back from other agents. */
export function skillCount(skills: Skill[]): string {
    const kept = skills.filter((skill) => !skill.shared).length;
    return kept > 0 ? `${skills.length}, ${kept} kept back` : `${skills.length}`;
}

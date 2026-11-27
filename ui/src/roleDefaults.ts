// The model and effort each role's sessions start on, as the settings choose them: the menus'
// choices and what a change keeps. Pure, so it can be tested; the core checks what is saved.

import type { AgentInfo, Effort, RoleDefault } from "./api/types";
import { modelLabel } from "./components/options";

/** The role defaults as kept, by role. */
export type RoleDefaults = Record<string, RoleDefault>;

/**
 * The models a role's sessions can be given, as `[value, label]`: none first, which leaves it to
 * the agent, then every model an agent offers, each once, and the one chosen now where no agent
 * offers it any longer, so that the menu still shows it.
 */
export function roleModelChoices(
    agents: AgentInfo[],
    current: string | null | undefined,
): [value: string, label: string][] {
    const models = new Set(agents.flatMap((agent) => agent.models));
    if (current) {
        models.add(current);
    }
    return [
        ["", "Agent's model"],
        ...[...models].map((model): [string, string] => [model, modelLabel(model)]),
    ];
}

/**
 * The role defaults once `role`'s model or effort is changed to `chosen`: an empty choice is
 * none, and a role left with neither is taken out, as the core keeps them.
 */
export function withRoleDefault(
    defaults: RoleDefaults | undefined,
    role: string,
    chosen: { model?: string; effort?: Effort | "" },
): RoleDefaults {
    const before = defaults?.[role] ?? {};
    const model = "model" in chosen ? chosen.model || null : (before.model ?? null);
    const effort = "effort" in chosen ? chosen.effort || null : (before.effort ?? null);
    const others = Object.entries(defaults ?? {}).filter(([name]) => name !== role);
    const now: RoleDefault = {};
    if (model) {
        now.model = model;
    }
    if (effort) {
        now.effort = effort;
    }
    const kept = model || effort ? [[role, now] as const] : [];
    return Object.fromEntries([...others, ...kept]);
}

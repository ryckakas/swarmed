import { api } from "../api";
import type { Effort, Settings } from "../api/types";
import { roleModelChoices, withRoleDefault } from "../roleDefaults";
import { useStore } from "../store";
import { useWords } from "../useWords";
import { capitalized } from "../wording";
import { EFFORT_CHOICES, useAgents, useRoles } from "./options";
import { Picker } from "./Picker";
import { Row } from "./SettingsRow";

/**
 * The model and effort each role's sessions start on, whoever starts them: the user, or an agent
 * with `spawn_session` or `start_task`. A choice made for the session wins. Agents cannot change
 * these.
 */
export function RoleDefaultsSettings({ settings }: { settings: Settings }) {
    const words = useWords();
    const change = useStore((state) => state.changeSettings);
    const agents = useAgents();
    const roles = useRoles();
    const defaults = settings.role_defaults;
    const keep = (role: string, chosen: { model?: string; effort?: Effort | "" }) =>
        void change(api, { role_defaults: withRoleDefault(defaults, role, chosen) });
    return (
        <section className="settings-group" aria-label="Role defaults">
            <h3>Role defaults</h3>
            <p className="settings-hint">
                The model and effort a new session with a role starts on, whether you start it or an{" "}
                {words.terms.agent} does, unless one is chosen for it. A model its{" "}
                {words.terms.agent} does not offer is passed over. Giving a session a role later
                changes neither. {capitalized(words.terms.agents)} cannot change these.
            </p>
            {roles.map((role) => {
                const chosen = defaults?.[role.name];
                const name = words.role(role.name);
                return (
                    <Row key={role.name} label={name} hint={capitalized(role.description)}>
                        <span className="role-default">
                            <Picker
                                label={`${name}: model`}
                                value={chosen?.model ?? ""}
                                onChange={(model) => keep(role.name, { model })}
                                options={roleModelChoices(agents, chosen?.model)}
                            />
                            <Picker
                                label={`${name}: effort`}
                                value={chosen?.effort ?? ""}
                                onChange={(effort) =>
                                    keep(role.name, { effort: effort as Effort | "" })
                                }
                                options={EFFORT_CHOICES}
                            />
                        </span>
                    </Row>
                );
            })}
        </section>
    );
}

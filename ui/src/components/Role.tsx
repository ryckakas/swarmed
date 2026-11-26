import { setRole } from "../actions";
import type { Session } from "../api/types";
import { roleAbout, roleChosen, roleOptions } from "../roles";
import { useWords } from "../useWords";
import { useRoles } from "./options";
import { Picker } from "./Picker";

/**
 * The menu that chooses what a session is for, or will be: its agent keeps to the role's rules,
 * from its start or, once something was said, from the next message.
 */
export function RoleMenu({
    value,
    onChange,
}: {
    value: string | null;
    onChange(role: string | null): void;
}) {
    const words = useWords();
    const roles = useRoles();
    const about = value === null ? "" : `, now ${roleAbout(value, roles, words)}`;
    return (
        <Picker
            label={`${words.roleMenu}: what this ${words.session} is for, whose rules its ${words.terms.agent} keeps to${about}`}
            value={value ?? ""}
            onChange={(chosen) => onChange(roleChosen(chosen))}
            options={roleOptions(roles, value, words)}
        />
    );
}

/** The role menu of a session that is there, in its header: a change applies at once. */
export function SessionRole({ session }: { session: Session }) {
    return <RoleMenu value={session.role} onChange={(role) => void setRole(session.id, role)} />;
}

/** A session's role, short, on its row in the sidebar; nothing for none. */
export function RoleChip({ role }: { role: string | null }) {
    const words = useWords();
    const roles = useRoles();
    if (role === null) {
        return null;
    }
    const about = roleAbout(role, roles, words);
    return (
        <span className="role-chip" title={about}>
            {words.roleShort(role)}
        </span>
    );
}

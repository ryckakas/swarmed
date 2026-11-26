import { setParent } from "../actions";
import { api } from "../api";
import type { Session } from "../api/types";
import { parentChosen, parentOptions } from "../parents";
import { useStore } from "../store";
import { childrenOf, findSession } from "../tree";
import { useWords } from "../useWords";
import { Picker } from "./Picker";
import { StatusMark } from "./StatusMark";

/**
 * The menu that chooses whom the session works for, of every other open session of its
 * repository: the sidebar lists it under that one, and both agents hear of it with their next
 * message.
 */
export function WorksForPicker({ session }: { session: Session }) {
    const words = useWords();
    const projects = useStore((state) => state.projects);
    return (
        <Picker
            label={`Whom this ${words.session} works for: it reports to that one, and is listed under it`}
            value={session.parent === null ? "" : String(session.parent)}
            onChange={(value) => void setParent(session.id, parentChosen(value))}
            options={parentOptions(projects, session, words)}
        />
    );
}

/**
 * Under the session's title, when anyone works with it: the session it works for, and those
 * that work for it, with how each is doing. Pressing one opens it. Nothing for a session that
 * works with nobody.
 */
export function WorkingWith({ session }: { session: Session }) {
    const words = useWords();
    const projects = useStore((state) => state.projects);
    const parent = session.parent === null ? undefined : findSession(projects, session.parent);
    const children = childrenOf(projects, session.id);
    if (!parent && children.length === 0) {
        return null;
    }
    return (
        <div className="working-with">
            {parent && (
                <p className="working-with-part">
                    <span className="working-with-label">{words.worksForHeading}</span>
                    <SessionChip session={parent} />
                </p>
            )}
            {children.length > 0 && (
                <p className="working-with-part">
                    <span className="working-with-label">{words.workingFor}</span>
                    {children.map((child) => (
                        <SessionChip key={child.id} session={child} />
                    ))}
                </p>
            )}
        </div>
    );
}

/** Another session, by its status and title, which opens it. */
function SessionChip({ session }: { session: Session }) {
    const words = useWords();
    const open = useStore((state) => state.open);
    const title = session.title || words.untitled;
    return (
        <button
            type="button"
            className="session-chip"
            title={`Open ${words.sessionNumbered(session.id).toLowerCase()}: ${title}`}
            onClick={() => void open(api, session.id)}
        >
            <StatusMark
                status={session.status}
                unread={session.unread}
                background={session.background}
            />
            <span className="session-chip-title">{title}</span>
        </button>
    );
}

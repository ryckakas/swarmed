import { type FormEvent, useEffect, useState } from "react";
import { actionLabel } from "../actionbar";
import { api } from "../api";
import type { Action } from "../api/types";
import { useStore } from "../store";
import { Picker } from "./Picker";

/** What an action does when its button is pressed. */
type Kind = "prompt" | "command";

/** An action as the form holds it while it is being written. */
interface Draft {
    /** The action being changed; `null` for a new one. */
    editing: string | null;
    name: string;
    icon: string;
    kind: Kind;
    text: string;
}

const NEW: Draft = { editing: null, name: "", icon: "", kind: "prompt", text: "" };

function draftOf(action: Action): Draft {
    const kind: Kind = action.command === "" ? "prompt" : "command";
    return {
        editing: action.name,
        name: action.name,
        icon: action.icon,
        kind,
        text: kind === "prompt" ? action.prompt : action.command,
    };
}

function actionOf(draft: Draft, ofProject: boolean): Action {
    return {
        name: draft.name.trim(),
        icon: draft.icon.trim(),
        prompt: draft.kind === "prompt" ? draft.text : "",
        command: draft.kind === "command" ? draft.text : "",
        of_project: ofProject,
    };
}

/** Where the actions on screen belong: every repository, or one, by its name. */
const EVERYWHERE = "";

/**
 * Buttons of the user's own, shown on a task: each sends a prompt to the agent or runs a command
 * in the session's terminal. Kept for every repository or for one.
 */
export function ActionsSettings() {
    const projects = useStore((state) => state.projects);
    const report = useStore((state) => state.report);
    const noteChanged = useStore((state) => state.noteActionsChanged);
    const [scope, setScope] = useState(EVERYWHERE);
    const [actions, setActions] = useState<Action[]>([]);
    const [draft, setDraft] = useState<Draft | null>(null);
    const project = scope === EVERYWHERE ? null : scope;

    useEffect(() => {
        let current = true;
        api.actions(project)
            .then((offered) => current && setActions(offered))
            .catch(useStore.getState().report);
        return () => {
            current = false;
        };
    }, [project]);

    const took = (offered: Action[]) => {
        setActions(offered);
        setDraft(null);
        noteChanged();
    };
    const save = async (saved: Draft) => {
        try {
            let now = await api.actionSave(project, actionOf(saved, project !== null));
            // Saved under a new name, the old one goes: it was renamed, not copied.
            if (saved.editing !== null && saved.editing !== saved.name.trim()) {
                now = await api.actionRemove(project, saved.editing);
            }
            took(now);
        } catch (error) {
            report(error);
        }
    };
    const remove = (name: string) => api.actionRemove(project, name).then(took).catch(report);
    // A repository's page lists its own; the general ones are changed on the general page.
    const own = actions.filter((action) => action.of_project === (project !== null));

    return (
        <section className="settings-group" aria-label="Actions">
            <h3>Actions</h3>
            <p className="settings-hint">
                Buttons of your own, shown on every task. Each sends a prompt to the agent of the
                session on screen, or runs a command in that session's terminal, in the task's
                worktree. They are kept as plain text in <code>actions.toml</code> in Swarmed's data
                folder, and a repository's own in <code>projects/NAME.actions.toml</code>, so you
                can also write them by hand, or ask an agent in any session to add one: it has a
                tool for that.
            </p>
            <div className="settings-row">
                <p className="settings-label">Shown in</p>
                <Picker
                    label="Where the actions are shown"
                    value={scope}
                    onChange={(chosen) => {
                        setScope(chosen);
                        setDraft(null);
                    }}
                    options={[
                        [EVERYWHERE, "Every repository"],
                        ...projects.map((one): [string, string] => [one.name, one.name]),
                    ]}
                />
            </div>
            {own.map((action) => (
                <div key={action.name} className="settings-row">
                    <div>
                        <p className="settings-label">{actionLabel(action)}</p>
                        {action.command === "" ? (
                            <p className="settings-hint template-preview">{action.prompt}</p>
                        ) : (
                            <p className="settings-hint template-preview mono">
                                $ {action.command}
                            </p>
                        )}
                    </div>
                    <button
                        type="button"
                        className="button"
                        onClick={() => setDraft(draftOf(action))}
                    >
                        Edit
                    </button>
                </div>
            ))}
            {draft ? (
                <ActionForm
                    draft={draft}
                    onChange={setDraft}
                    onSave={() => void save(draft)}
                    onRemove={
                        draft.editing === null ? null : () => void remove(draft.editing ?? "")
                    }
                    onCancel={() => setDraft(null)}
                />
            ) : (
                <button type="button" className="button" onClick={() => setDraft(NEW)}>
                    Add an action
                </button>
            )}
        </section>
    );
}

interface FormProps {
    draft: Draft;
    onChange(draft: Draft): void;
    onSave(): void;
    /** `null` for an action that is not saved yet. */
    onRemove: (() => void) | null;
    onCancel(): void;
}

function ActionForm({ draft, onChange, onSave, onRemove, onCancel }: FormProps) {
    const submit = (event: FormEvent) => {
        event.preventDefault();
        onSave();
    };
    const isPrompt = draft.kind === "prompt";
    return (
        <form className="environment-form" onSubmit={submit}>
            <label>
                <span className="settings-label">Name</span>
                <span className="settings-hint">What the button says.</span>
                <input
                    className="key-input"
                    aria-label="Name of the action"
                    placeholder="Run the tests"
                    maxLength={40}
                    required
                    value={draft.name}
                    onChange={(event) => onChange({ ...draft, name: event.target.value })}
                />
            </label>
            <label>
                <span className="settings-label">Icon</span>
                <span className="settings-hint">An emoji or a letter, or nothing.</span>
                <input
                    className="key-input action-icon-input"
                    aria-label="Icon of the action"
                    placeholder="🧪"
                    maxLength={8}
                    value={draft.icon}
                    onChange={(event) => onChange({ ...draft, icon: event.target.value })}
                />
            </label>
            <div className="settings-row">
                <p className="settings-label">When pressed</p>
                <Picker
                    label="What the action does"
                    value={draft.kind}
                    onChange={(kind) => onChange({ ...draft, kind: kind as Kind })}
                    options={[
                        ["prompt", "Send a prompt to the agent"],
                        ["command", "Run a command in the terminal"],
                    ]}
                />
            </div>
            <label>
                <span className="settings-label">{isPrompt ? "Prompt" : "Command"}</span>
                <textarea
                    className={isPrompt ? "key-input" : "key-input mono"}
                    aria-label={isPrompt ? "The prompt" : "The command"}
                    placeholder={
                        isPrompt
                            ? "Run the end-to-end tests and mark this session green if they pass."
                            : "nix develop"
                    }
                    rows={isPrompt ? 6 : 3}
                    spellCheck={isPrompt}
                    required
                    value={draft.text}
                    onChange={(event) => onChange({ ...draft, text: event.target.value })}
                />
            </label>
            <div className="confirm-actions">
                {onRemove && (
                    <button type="button" className="button environment-remove" onClick={onRemove}>
                        Remove
                    </button>
                )}
                <button type="button" className="button" onClick={onCancel}>
                    Cancel
                </button>
                <button type="submit" className="button button-primary">
                    Save action
                </button>
            </div>
        </form>
    );
}

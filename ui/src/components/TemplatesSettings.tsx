import { type FormEvent, useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import { useWords } from "../useWords";
import { capitalized, withArticle } from "../wording";

/** A prompt as the form holds it while it is being written. */
interface Draft {
    /** The prompt being changed; `null` for a new one. */
    editing: string | null;
    name: string;
    text: string;
}

const NEW: Draft = { editing: null, name: "", text: "" };

/** Prompts kept to be sent again: typed `/name` in a message box puts one in. */
export function TemplatesSettings() {
    const words = useWords();
    const templates = useStore((state) => state.templates);
    const setTemplates = useStore((state) => state.setTemplates);
    const report = useStore((state) => state.report);
    const [draft, setDraft] = useState<Draft | null>(null);

    const save = async (saved: Draft) => {
        try {
            let now = await api.templateSave(saved.name.trim(), saved.text);
            // Saved under a new name, the old one goes: it was renamed, not copied.
            if (saved.editing !== null && saved.editing !== saved.name.trim()) {
                now = await api.templateRemove(saved.editing);
            }
            setTemplates(now);
            setDraft(null);
        } catch (error) {
            report(error);
        }
    };
    const remove = (name: string) =>
        api
            .templateRemove(name)
            .then(setTemplates)
            .then(() => setDraft(null))
            .catch(report);

    return (
        <section className="settings-group" aria-label={capitalized(words.terms.prompts)}>
            <h3>{capitalized(words.terms.prompts)}</h3>
            <p className="settings-hint">
                Prompts you send often, kept under a name. Type / and the name in any message box to
                put one in, whatever the agent. Write {"{{something}}"} where a prompt has a place
                to fill in: the first one is selected, ready to type over.
            </p>
            {templates.map((template) => (
                <div key={template.name} className="settings-row">
                    <div>
                        <p className="settings-label mono">/{template.name}</p>
                        <p className="settings-hint template-preview">{template.text}</p>
                    </div>
                    <button
                        type="button"
                        className="button"
                        onClick={() => setDraft({ editing: template.name, ...template })}
                    >
                        Edit
                    </button>
                </div>
            ))}
            {draft ? (
                <TemplateForm
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
                    Add {withArticle(words.terms.prompt)}
                </button>
            )}
        </section>
    );
}

interface FormProps {
    draft: Draft;
    onChange(draft: Draft): void;
    onSave(): void;
    /** `null` for a prompt that is not saved yet. */
    onRemove: (() => void) | null;
    onCancel(): void;
}

function TemplateForm({ draft, onChange, onSave, onRemove, onCancel }: FormProps) {
    const words = useWords();
    const submit = (event: FormEvent) => {
        event.preventDefault();
        onSave();
    };
    return (
        <form className="environment-form" onSubmit={submit}>
            <label>
                <span className="settings-label">Name</span>
                <span className="settings-hint">Lower-case letters, digits and dashes.</span>
                <input
                    className="key-input mono"
                    aria-label={`Name of the ${words.terms.prompt}`}
                    placeholder="review"
                    spellCheck={false}
                    required
                    value={draft.name}
                    onChange={(event) => onChange({ ...draft, name: event.target.value })}
                />
            </label>
            <label>
                <span className="settings-label">{capitalized(words.terms.prompt)}</span>
                <textarea
                    className="key-input"
                    aria-label={`The ${words.terms.prompt}`}
                    placeholder="Review {{what}} for bugs and say what you would change."
                    rows={8}
                    required
                    value={draft.text}
                    onChange={(event) => onChange({ ...draft, text: event.target.value })}
                />
            </label>
            <div className="form-actions">
                {onRemove && (
                    <button type="button" className="button environment-remove" onClick={onRemove}>
                        Remove
                    </button>
                )}
                <button type="button" className="button" onClick={onCancel}>
                    Cancel
                </button>
                <button type="submit" className="button button-primary">
                    Save {words.terms.prompt}
                </button>
            </div>
        </form>
    );
}

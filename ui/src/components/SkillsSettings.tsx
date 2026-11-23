import { type FormEvent, useEffect, useState } from "react";
import { api } from "../api";
import type { Skill } from "../api/types";
import { iconForAgent } from "../providerIcons";
import { SKILL_ORIGINS, skillCount, skillsOf } from "../skills";
import { useStore } from "../store";
import { useWords } from "../useWords";
import { capitalized, withArticle } from "../wording";
import { ProviderMark } from "./ProviderMark";

/** A skill of Swarmed's own as the form holds it while it is being written. */
interface Draft {
    /** The skill being changed; `null` for a new one. */
    editing: string | null;
    name: string;
    description: string;
    body: string;
}

const NEW: Draft = { editing: null, name: "", description: "", body: "" };

/**
 * Skills: the ones kept in Swarmed, which every session is offered, and each agent's own,
 * which sessions on other agents are offered unless they are kept back.
 */
export function SkillsSettings() {
    const words = useWords();
    const report = useStore((state) => state.report);
    const [skills, setSkills] = useState<Skill[]>([]);
    const [draft, setDraft] = useState<Draft | null>(null);
    // biome-ignore lint/correctness/useExhaustiveDependencies: read once, when the tab opens
    useEffect(() => {
        api.skills().then(setSkills).catch(report);
    }, []);

    const own = skillsOf(skills, "swarmed");
    const edit = (skill: Skill) =>
        api
            .skillBody(skill.name)
            .then((body) =>
                setDraft({
                    editing: skill.name,
                    name: skill.name,
                    description: skill.description,
                    body,
                }),
            )
            .catch(report);
    const save = async (saved: Draft) => {
        try {
            let now = await api.skillSave(saved.name.trim(), saved.description, saved.body);
            // Saved under a new name, the old one goes: it was renamed, not copied.
            if (saved.editing !== null && saved.editing !== saved.name.trim()) {
                now = await api.skillRemove(saved.editing);
            }
            setSkills(now);
            setDraft(null);
        } catch (error) {
            report(error);
        }
    };
    const remove = (name: string) =>
        api
            .skillRemove(name)
            .then(setSkills)
            .then(() => setDraft(null))
            .catch(report);
    const share = (skill: Skill, shared: boolean) =>
        api.skillShare(skill.origin, skill.name, shared).then(setSkills).catch(report);

    return (
        <section className="settings-group" aria-label={capitalized(words.terms.skills)}>
            <h3>{capitalized(words.terms.skills)}</h3>
            <p className="settings-hint">
                A skill is a set of instructions for one kind of task. Every agent keeps its own and
                knows nothing of the others'. Swarmed reads them all and offers each session the
                ones its agent lacks: they are named to the agent when it starts, and handed over
                when it asks. Nothing is copied into an agent's own folders.
            </p>
            {own.map((skill) => (
                <div key={skill.name} className="settings-row">
                    <div>
                        <p className="settings-label mono">{skill.name}</p>
                        <p className="settings-hint">{skill.description}</p>
                    </div>
                    <button type="button" className="button" onClick={() => void edit(skill)}>
                        Edit
                    </button>
                </div>
            ))}
            {draft ? (
                <SkillForm
                    draft={draft}
                    onChange={setDraft}
                    onSave={() => void save(draft)}
                    onRemove={draft.editing === null ? null : () => void remove(draft.name)}
                    onCancel={() => setDraft(null)}
                />
            ) : (
                <button type="button" className="button" onClick={() => setDraft(NEW)}>
                    Add {withArticle(words.terms.skill)}
                </button>
            )}
            <p className="settings-hint">
                A repository can keep skills of its own in .claude/skills or .agents/skills. Those
                go with its tasks: a session is offered the ones in its worktree that its agent does
                not read by itself.
            </p>
            {SKILL_ORIGINS.map((group) => (
                <details key={group.origin} className="mcp-group">
                    <summary>
                        <span className="mcp-group-fold" aria-hidden="true">
                            ›
                        </span>
                        <ProviderMark icon={iconForAgent(group.agent)} />
                        <span className="mcp-group-name">{group.name}</span>
                        <span className="mcp-group-count">
                            {skillCount(skillsOf(skills, group.origin))}
                        </span>
                    </summary>
                    <p className="settings-hint">{group.about}</p>
                    <ul className="mcp-list">
                        {skillsOf(skills, group.origin).map((skill) => (
                            <li key={skill.name} className="skill-row">
                                <label
                                    title={`Offer it to sessions on other ${words.terms.agents}`}
                                >
                                    <input
                                        type="checkbox"
                                        checked={skill.shared}
                                        onChange={(event) =>
                                            void share(skill, event.target.checked)
                                        }
                                    />
                                    <span className="mono">{skill.name}</span>
                                </label>
                                <span className="skill-about" title={skill.description}>
                                    {skill.description}
                                </span>
                            </li>
                        ))}
                    </ul>
                </details>
            ))}
        </section>
    );
}

interface FormProps {
    draft: Draft;
    onChange(draft: Draft): void;
    onSave(): void;
    /** `null` for a skill that is not saved yet. */
    onRemove: (() => void) | null;
    onCancel(): void;
}

function SkillForm({ draft, onChange, onSave, onRemove, onCancel }: FormProps) {
    const words = useWords();
    const set = (change: Partial<Draft>) => onChange({ ...draft, ...change });
    const submit = (event: FormEvent) => {
        event.preventDefault();
        onSave();
    };
    return (
        <form className="environment-form" onSubmit={submit}>
            <label>
                <span className="settings-label">Name</span>
                <input
                    className="key-input mono"
                    aria-label={`Name of the ${words.terms.skill}`}
                    placeholder="release-notes"
                    spellCheck={false}
                    required
                    value={draft.name}
                    onChange={(event) => set({ name: event.target.value })}
                />
            </label>
            <label>
                <span className="settings-label">When to use it</span>
                <span className="settings-hint">
                    One or two sentences. This is all an agent sees until it asks for the skill.
                </span>
                <input
                    className="key-input"
                    aria-label={`When to use the ${words.terms.skill}`}
                    placeholder="Write release notes. Use when the user asks for a changelog or release notes."
                    required
                    value={draft.description}
                    onChange={(event) => set({ description: event.target.value })}
                />
            </label>
            <label>
                <span className="settings-label">What it says to do</span>
                <textarea
                    className="key-input"
                    aria-label={`What the ${words.terms.skill} says to do`}
                    rows={10}
                    required
                    value={draft.body}
                    onChange={(event) => set({ body: event.target.value })}
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
                    Save {words.terms.skill}
                </button>
            </div>
        </form>
    );
}

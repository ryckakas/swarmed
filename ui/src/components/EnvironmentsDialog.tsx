import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Environment } from "../api/types";
import {
    EMPTY_ENVIRONMENT,
    envFromText,
    envToText,
    portsFromText,
    portsToText,
} from "../environments";
import { useStore } from "../store";
import { useWords } from "../useWords";
import { withArticle } from "../wording";
import { Snapshots } from "./Snapshots";

/** A repository's environments, opened from the sidebar. */
export function EnvironmentsDialog() {
    const project = useStore((state) => state.environmentsOf);
    if (project === null) {
        return null;
    }
    // A key per repository, so one's environments never show under another's name.
    return <Editor key={project} project={project} />;
}

/** An environment as the form holds it while it is being written. */
interface Draft {
    /** The environment being changed; `null` for a new one. */
    editing: string | null;
    name: string;
    setup: string;
    start: string;
    stop: string;
    teardown: string;
    openUrl: string;
    /** The variables, as lines of `NAME=value`. */
    variables: string;
    /** The ports, as lines of `name=3000`. */
    ports: string;
}

const NEW: Draft = {
    editing: null,
    name: "",
    setup: "",
    start: "",
    stop: "",
    teardown: "",
    openUrl: "",
    variables: "",
    ports: "",
};

function draftOf(name: string, environment: Environment): Draft {
    return {
        editing: name,
        name,
        setup: environment.setup,
        start: environment.start,
        stop: environment.stop,
        teardown: environment.teardown,
        openUrl: environment.open_url,
        variables: envToText(environment.env),
        ports: portsToText(environment.ports ?? {}),
    };
}

/** What is saved for a draft. Throws, naming the line, if a variable is not written as one. */
function environmentOf(draft: Draft): Environment {
    return {
        ...EMPTY_ENVIRONMENT,
        setup: draft.setup.trim(),
        start: draft.start.trim(),
        stop: draft.stop.trim(),
        teardown: draft.teardown.trim(),
        open_url: draft.openUrl.trim(),
        ports: portsFromText(draft.ports),
        env: envFromText(draft.variables),
    };
}

function Editor({ project }: { project: string }) {
    const words = useWords();
    const close = () => useStore.getState().askForEnvironments(null);
    const dialog = useRef<HTMLDialogElement>(null);
    const [environments, setEnvironments] = useState<Record<string, Environment>>({});
    const [draft, setDraft] = useState<Draft | null>(null);
    const [problem, setProblem] = useState<string | null>(null);
    const failed = (error: unknown) => setProblem(String(error));
    const read = () => api.environments(project).then(setEnvironments, failed);

    // biome-ignore lint/correctness/useExhaustiveDependencies: read once, when the dialog opens
    useEffect(() => {
        dialog.current?.showModal();
        void read();
    }, []);

    const save = async (saved: Draft) => {
        try {
            await api.environmentSave(project, saved.name.trim(), environmentOf(saved));
            // Saved under a new name, the old one goes: it was renamed, not copied.
            if (saved.editing !== null && saved.editing !== saved.name.trim()) {
                await api.environmentRemove(project, saved.editing);
            }
            setProblem(null);
            setDraft(null);
            await read();
        } catch (error) {
            failed(error);
        }
    };
    const remove = (name: string) =>
        api
            .environmentRemove(project, name)
            .then(() => setDraft(null))
            .then(read, failed);
    const names = Object.keys(environments);

    return (
        <dialog
            ref={dialog}
            className="confirm environments"
            aria-labelledby="environments-title"
            onClose={close}
        >
            <h2 id="environments-title">
                {words.environments} of {project}
            </h2>
            <p>
                Different ways of setting a task up on this Mac: what runs when its worktree is
                made, and the variables its hooks and agents get. One is chosen when a task is
                started. Whatever one leaves empty is as the repository's own config has it.
            </p>
            {draft === null ? (
                <Listing
                    names={names}
                    onEdit={(name) => {
                        const found = environments[name];
                        setDraft(found ? draftOf(name, found) : NEW);
                    }}
                    onNew={() => setDraft(NEW)}
                />
            ) : (
                <Form
                    draft={draft}
                    onChange={setDraft}
                    onSave={() => void save(draft)}
                    onRemove={draft.editing === null ? null : () => void remove(draft.name)}
                    onCancel={() => setDraft(null)}
                />
            )}
            {problem && <p className="note-problem">{problem}</p>}
            {draft === null && <Snapshots project={project} environments={names} />}
            {draft === null && (
                <div className="confirm-actions">
                    <button type="button" className="button button-primary" onClick={close}>
                        Done
                    </button>
                </div>
            )}
        </dialog>
    );
}

function Listing(props: { names: string[]; onEdit(name: string): void; onNew(): void }) {
    const words = useWords();
    return (
        <div className="environment-list">
            {props.names.length === 0 && (
                <p className="settings-hint">
                    None yet. Tasks are set up the way the repository's own config says.
                </p>
            )}
            {props.names.map((name) => (
                <button
                    key={name}
                    type="button"
                    className="environment-name mono"
                    onClick={() => props.onEdit(name)}
                >
                    {name}
                </button>
            ))}
            <button type="button" className="button" onClick={props.onNew}>
                Add {withArticle(words.environment)}
            </button>
        </div>
    );
}

interface FormProps {
    draft: Draft;
    onChange(draft: Draft): void;
    onSave(): void;
    /** `null` for an environment that is not saved yet. */
    onRemove: (() => void) | null;
    onCancel(): void;
}

function Form({ draft, onChange, onSave, onRemove, onCancel }: FormProps) {
    const words = useWords();
    const set = (change: Partial<Draft>) => onChange({ ...draft, ...change });
    return (
        <form
            className="environment-form"
            onSubmit={(event) => {
                event.preventDefault();
                onSave();
            }}
        >
            <label>
                <span className="settings-label">Name</span>
                <input
                    className="key-input mono"
                    aria-label={`Name of the ${words.environment.toLowerCase()}`}
                    placeholder="staging"
                    spellCheck={false}
                    required
                    value={draft.name}
                    onChange={(event) => set({ name: event.target.value })}
                />
            </label>
            <Script
                label="Setup"
                hint="Runs once in a new task's worktree and has to finish before the agent starts. Not for anything that keeps running."
                placeholder="npm ci && npm run db:seed"
                value={draft.setup}
                onChange={(setup) => set({ setup })}
            />
            <Script
                label="Start"
                hint="Started when setup is done and left running in the background: a dev server, devenv up. Started and stopped from the task's panel, and stopped when the task is removed or Swarmed quits."
                placeholder="devenv up"
                value={draft.start}
                onChange={(start) => set({ start })}
            />
            <Script
                label="Ports"
                hint="One name=number per line. Each task gets its own number for a port, so stacks can run side by side: write {{port.web}} in the commands, variables and address instead of the number."
                placeholder={"web=3000\ndb=5432"}
                value={draft.ports}
                onChange={(ports) => set({ ports })}
            />
            <Script
                label="Variables"
                hint="One NAME=value per line, for the hooks and for the agents working in the task. Kept in a file on this Mac that only you can read."
                placeholder={"DATABASE_URL=postgres://localhost/{{task}}\nAPP_ENV=staging"}
                value={draft.variables}
                onChange={(variables) => set({ variables })}
            />
            <Script
                label="Stop"
                hint="Optional. Runs when the stack is stopped, before what Start left running is ended."
                placeholder="docker compose stop"
                value={draft.stop}
                onChange={(stop) => set({ stop })}
            />
            <Script
                label="Teardown"
                hint="Runs before a task's worktree is removed, in place of the repository's teardown hook."
                placeholder="docker compose down -v"
                value={draft.teardown}
                onChange={(teardown) => set({ teardown })}
            />
            <label>
                <span className="settings-label">Address</span>
                <span className="settings-hint">
                    What "Open in browser" opens for a task. Left empty, the addresses on this
                    machine that the stack prints are offered once they answer.
                </span>
                <input
                    className="key-input mono"
                    aria-label="Address of the running task"
                    placeholder="http://localhost:{{port.web}}"
                    spellCheck={false}
                    value={draft.openUrl}
                    onChange={(event) => set({ openUrl: event.target.value })}
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
                    Save {words.environment.toLowerCase()}
                </button>
            </div>
        </form>
    );
}

interface ScriptProps {
    label: string;
    hint: string;
    placeholder: string;
    value: string;
    onChange(value: string): void;
}

/** A few lines of commands or variables, in the type they are run in. */
function Script({ label, hint, placeholder, value, onChange }: ScriptProps) {
    return (
        <label>
            <span className="settings-label">{label}</span>
            <span className="settings-hint">{hint}</span>
            <textarea
                className="key-input mono"
                aria-label={label}
                rows={3}
                spellCheck={false}
                placeholder={placeholder}
                value={value}
                onChange={(event) => onChange(event.target.value)}
            />
        </label>
    );
}

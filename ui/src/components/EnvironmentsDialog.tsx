import { useEffect, useState } from "react";
import { api } from "../api";
import type { Environment } from "../api/types";
import {
    EMPTY_ENVIRONMENT,
    envFromText,
    envToText,
    OWN_SETUP,
    portsFromText,
    portsToText,
    setupsFromText,
    stacksFromText,
    stacksToText,
} from "../environments";
import { useStore } from "../store";
import { useWords } from "../useWords";
import { capitalized, withArticle } from "../wording";
import { Dialog } from "./Dialog";
import { Picker } from "./Picker";
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
    /** How many stacks may run at once, as typed; empty for the repository's own limit. */
    maxStacks: string;
    /** How many setups may run at once, as typed; empty for the repository's own limit. */
    maxSetups: string;
}

/** The form's id, so that the buttons below what scrolls can submit it. */
const FORM_ID = "environment-form";

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
    maxStacks: "",
    maxSetups: "",
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
        maxStacks: stacksToText(environment.max_stacks),
        maxSetups: stacksToText(environment.max_setups),
    };
}

/** What is saved for a draft. Throws, naming the line or value, if one is not written as it has to be. */
function environmentOf(draft: Draft): Environment {
    const maxStacks = stacksFromText(draft.maxStacks);
    const maxSetups = setupsFromText(draft.maxSetups);
    return {
        ...EMPTY_ENVIRONMENT,
        setup: draft.setup.trim(),
        start: draft.start.trim(),
        stop: draft.stop.trim(),
        teardown: draft.teardown.trim(),
        open_url: draft.openUrl.trim(),
        ports: portsFromText(draft.ports),
        env: envFromText(draft.variables),
        ...(maxStacks === undefined ? {} : { max_stacks: maxStacks }),
        ...(maxSetups === undefined ? {} : { max_setups: maxSetups }),
    };
}

/**
 * Saves what the form holds. Saved under a new name, the old one goes: it was renamed, not
 * copied. Removing the default leaves none, so a renamed default is made the default again.
 */
async function saveDraft(project: string, draft: Draft, usual: string | null) {
    const name = draft.name.trim();
    await api.environmentSave(project, name, environmentOf(draft));
    if (draft.editing === null || draft.editing === name) {
        return;
    }
    await api.environmentRemove(project, draft.editing);
    if (draft.editing === usual) {
        await api.environmentSetDefault(project, name);
    }
}

function Editor({ project }: { project: string }) {
    const words = useWords();
    const close = () => useStore.getState().askForEnvironments(null);
    const [environments, setEnvironments] = useState<Record<string, Environment>>({});
    // The one new tasks are set up in unless another is chosen.
    const [usual, setUsual] = useState<string | null>(null);
    const [draft, setDraft] = useState<Draft | null>(null);
    const [problem, setProblem] = useState<string | null>(null);
    const failed = (error: unknown) => setProblem(String(error));
    const read = () =>
        Promise.all([api.environments(project), api.environmentDefault(project)]).then(
            ([kept, chosen]) => {
                setEnvironments(kept);
                setUsual(chosen);
            },
            failed,
        );

    // biome-ignore lint/correctness/useExhaustiveDependencies: read once, when the dialog opens
    useEffect(() => {
        void read();
    }, []);

    const save = async (saved: Draft) => {
        try {
            await saveDraft(project, saved, usual);
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
    const makeDefault = (name: string | null) =>
        api.environmentSetDefault(project, name).then(read, failed);
    const names = Object.keys(environments);
    // The name it is saved under, whatever the form's name field says by now.
    const saved = draft?.editing ?? null;

    const editing = draft !== null;
    // Over the form, the footer's buttons are the form's own, and what was typed would be lost
    // by a click beside the dialog: only Esc and the × close it then.
    return (
        <Dialog
            title={`${words.environments} of ${project}`}
            size="medium"
            closeOnBackdrop={!editing}
            onClose={close}
            problem={problem}
            dismiss={editing ? null : "Done"}
            aside={
                editing &&
                saved !== null && (
                    <button type="button" className="button" onClick={() => void remove(saved)}>
                        Remove
                    </button>
                )
            }
            actions={editing && <FormButtons onCancel={() => setDraft(null)} />}
        >
            <p>
                Different ways of setting a task up on this Mac: what runs when its worktree is
                made, and the variables its hooks and agents get. A new task is set up in the
                default one unless another is chosen when it is started. Whatever one leaves empty
                is as the repository's own config has it.
            </p>
            {draft === null ? (
                <Listing
                    names={names}
                    usual={usual}
                    onDefault={(name) => void makeDefault(name)}
                    onEdit={(name) => {
                        const found = environments[name];
                        setDraft(found ? draftOf(name, found) : NEW);
                    }}
                    onNew={() => setDraft(NEW)}
                />
            ) : (
                <Form draft={draft} onChange={setDraft} onSave={() => void save(draft)} />
            )}
            {draft === null && <Snapshots project={project} environments={names} />}
        </Dialog>
    );
}

interface ListingProps {
    names: string[];
    /** The default one; `null` when new tasks get the repository's own setup. */
    usual: string | null;
    onDefault(name: string | null): void;
    onEdit(name: string): void;
    onNew(): void;
}

function Listing(props: ListingProps) {
    const words = useWords();
    return (
        <>
            <div className="environment-list">
                {props.names.length === 0 && (
                    <p className="settings-hint">
                        None yet. Tasks are set up the way the repository's own config says.
                    </p>
                )}
                {/* The keyboard starts on the first of these: an environment, or adding one. */}
                {props.names.map((name) => (
                    <button
                        key={name}
                        type="button"
                        className="environment-name mono"
                        onClick={() => props.onEdit(name)}
                        data-dialog-start
                    >
                        {name}
                    </button>
                ))}
                <button type="button" className="button" onClick={props.onNew} data-dialog-start>
                    Add {withArticle(words.environment)}
                </button>
            </div>
            {props.names.length > 0 && (
                <DefaultPicker names={props.names} usual={props.usual} onChange={props.onDefault} />
            )}
        </>
    );
}

/** Which environment new tasks are set up in when none is chosen as they start. */
function DefaultPicker(props: {
    names: string[];
    usual: string | null;
    onChange(name: string | null): void;
}) {
    const words = useWords();
    const label = `Default ${words.environment.toLowerCase()}`;
    // The menu names itself: a label around a component would not be tied to its control.
    return (
        <div className="environment-default">
            <span className="settings-label">{label}</span>
            <span className="settings-hint">
                What a new {words.terms.task} is set up in unless another is chosen for it.{" "}
                {capitalized(words.terms.task)}s already made keep theirs.
            </span>
            <Picker
                label={label}
                value={props.usual ?? OWN_SETUP}
                onChange={(value) => props.onChange(value === OWN_SETUP ? null : value)}
                options={[
                    [OWN_SETUP, words.ownSetup],
                    ...props.names.map((name): [string, string] => [name, name]),
                ]}
            />
        </div>
    );
}

/** The form's own buttons, in the dialog's footer, so that they stay in view. */
function FormButtons({ onCancel }: { onCancel(): void }) {
    const words = useWords();
    return (
        <>
            <button type="button" className="button" onClick={onCancel}>
                Cancel
            </button>
            <button type="submit" form={FORM_ID} className="button button-primary">
                Save {words.environment.toLowerCase()}
            </button>
        </>
    );
}

interface FormProps {
    draft: Draft;
    onChange(draft: Draft): void;
    onSave(): void;
}

function Form({ draft, onChange, onSave }: FormProps) {
    const words = useWords();
    const set = (change: Partial<Draft>) => onChange({ ...draft, ...change });
    return (
        <form
            id={FORM_ID}
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
            <label>
                <span className="settings-label">Stacks at once</span>
                <span className="settings-hint">
                    How many of the repository's stacks may be running when a {words.terms.task} in
                    this {words.environment.toLowerCase()} starts its own; one more is refused until
                    another is stopped. Left empty, the repository's own limit applies.
                </span>
                <input
                    className="key-input mono environment-stacks"
                    aria-label="Stacks at once"
                    type="number"
                    inputMode="numeric"
                    min={1}
                    step={1}
                    value={draft.maxStacks}
                    onChange={(event) => set({ maxStacks: event.target.value })}
                />
            </label>
            <label>
                <span className="settings-label">Setups at once</span>
                <span className="settings-hint">
                    How many of the repository's {words.terms.task}s may run setup at once for one
                    in this {words.environment.toLowerCase()} to run its own; the others wait for a
                    place, and say so. 0 is no limit. Left empty, the repository's own limit
                    applies.
                </span>
                <input
                    className="key-input mono environment-stacks"
                    aria-label="Setups at once"
                    type="number"
                    inputMode="numeric"
                    min={0}
                    step={1}
                    value={draft.maxSetups}
                    onChange={(event) => set({ maxSetups: event.target.value })}
                />
            </label>
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
                    aria-label={`Address of the running ${words.terms.task}`}
                    placeholder="http://localhost:{{port.web}}"
                    spellCheck={false}
                    value={draft.openUrl}
                    onChange={(event) => set({ openUrl: event.target.value })}
                />
            </label>
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

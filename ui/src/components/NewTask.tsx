import { useEffect, useState } from "react";
import { api } from "../api";
import type {
    AgentInfo,
    BranchChoices,
    Effort,
    Project,
    SessionSetup,
    Supervision,
    TaskNaming,
} from "../api/types";
import {
    type Variant,
    variantChoices,
    variantFrom,
    variantKey,
    variantLabel,
    variantNames,
} from "../broadcast";
import { expandPastes, forgetPastes } from "../pastes";
import { sentOn } from "../slash";
import { useStore } from "../store";
import { taskNameForPrompt, taskNameFrom, uniqueTaskName } from "../tree";
import { useWords } from "../useWords";
import type { Words } from "../wording";
import { wordsFor } from "../wording";
import { AgentNotice } from "./AgentNotice";
import { ComposerBox } from "./ComposerBox";
import { EffortPicker } from "./EffortPicker";
import { FastToggle } from "./FastToggle";
import { Incubating } from "./Incubating";
import { ModePicker } from "./ModePicker";
import {
    CLAUDE,
    modelChoices,
    takesEffort,
    takesFast,
    useAgents,
    useEffortDefault,
    useSlashCommands,
} from "./options";
import { Picker } from "./Picker";

/** The value of the branch picker that means "create a new branch". */
const NEW_BRANCH = "";

/** Starts a task from a prompt: pick the repository and branch, type, send. */
export function NewTask({ projects }: { projects: Project[] }) {
    const draftProject = useStore((state) => state.draftProject);
    const hookLine = useStore((state) => state.hookLine);
    const words = useStore((state) => wordsFor(state.settings?.style));
    const [chosen, setChosen] = useState<string | null>(null);
    const [branch, setBranch] = useState(NEW_BRANCH);
    // The menu starts on the default from the settings and keeps what the user picks.
    const defaultSupervision = useStore((state) => state.settings?.default_supervision);
    const [pickedSupervision, setSupervision] = useState<Supervision | null>(null);
    const supervision = pickedSupervision ?? defaultSupervision ?? "auto";
    const [agentId, setAgentId] = useState(CLAUDE);
    const [model, setModel] = useState("");
    const [effort, setEffort] = useState<Effort | null>(null);
    const [fast, setFast] = useState(false);
    // More agents to run the same prompt on, each in a task of its own.
    const [variants, setVariants] = useState<Variant[]>([]);
    const agents = useAgents();
    // A task starts with a message to its agent: a screen or a setting has no session yet.
    const commands = sentOn(useSlashCommands(null));
    const [prompt, setPrompt] = useState("");
    const [files, setFiles] = useState<string[]>([]);
    const [creating, setCreating] = useState<string | null>(null);

    // The sidebar's plus on a project preselects it; otherwise the last choice, or the first.
    const project =
        projects.find((candidate) => candidate.name === (chosen ?? draftProject)) ?? projects[0];
    const choices = useBranchChoices(project?.name);
    const environments = useEnvironments(project?.name);
    const [environment, setEnvironment] = useState("");
    if (!project) {
        return null;
    }

    // A removed task's name too, since its branch may still be there. The core has the last
    // word: it steps past branches here and on a remote, and the task it makes says its name.
    const taken = project.tasks.map((task) => task.name);
    const base = branch === NEW_BRANCH ? taskNameForPrompt(prompt) : taskNameFrom(branch);
    const name = uniqueTaskName(base || "task", taken);
    const canSend = prompt.trim() !== "" && creating === null;

    const send = async () => {
        if (!canSend) {
            return;
        }
        setCreating(NAMING);
        useStore.setState({ hookLine: null });
        try {
            // A task on a new branch is named for what it is about; one on an existing branch
            // keeps that branch's name.
            const naming = branch === NEW_BRANCH ? await named(project.name, prompt.trim()) : null;
            const chosen = naming ? uniqueTaskName(naming.name, taken) : name;
            setCreating(chosen);
            const start = {
                project: project.name,
                name: chosen,
                title: naming?.title ?? null,
                branch: branch === NEW_BRANCH ? null : branch,
                environment: chosenEnvironment(environments, environment),
                setup: setupFor(agentId, model, supervision, effort, fast),
                prompt: expandPastes(prompt.trim()),
                files,
            };
            await startTask(start, true);
            await startVariants(start, variants, taken, { supervision, effort });
            forgetPastes(prompt);
        } catch (error) {
            // A task whose setup failed still exists; reload so it shows up in the sidebar.
            const { report, refresh } = useStore.getState();
            report(error);
            await refresh(api);
            setCreating(null);
        }
    };

    return (
        <div className="new-task">
            <h1>{words.newTask}</h1>
            <ComposerBox
                value={prompt}
                onChange={setPrompt}
                files={files}
                onFiles={setFiles}
                onSend={() => void send()}
                canSend={canSend}
                placeholder={words.describeTask}
                disabled={creating !== null}
                autoFocus
                commands={commands}
            >
                <Picker
                    label={words.repository}
                    value={project.name}
                    onChange={(value) => {
                        setChosen(value);
                        setBranch(NEW_BRANCH);
                    }}
                    options={projects.map((candidate) => [candidate.name, candidate.name])}
                />
                <Picker
                    label="Branch"
                    value={branch}
                    onChange={setBranch}
                    options={[
                        [NEW_BRANCH, `New branch from ${choices?.base ?? "the default branch"}`],
                        ...(choices?.existing ?? []).map((existing): [string, string] => [
                            existing,
                            existing,
                        ]),
                    ]}
                />
                <EnvironmentPicker
                    names={environments}
                    value={environment}
                    onChange={setEnvironment}
                />
                <ModePicker value={supervision} agentId={agentId} onChange={setSupervision} />
                <AgentPickers
                    agents={agents}
                    agentId={agentId}
                    model={model}
                    onAgent={(value) => {
                        setAgentId(value);
                        setModel("");
                    }}
                    onModel={setModel}
                    effort={effort}
                    onEffort={setEffort}
                    fast={fast}
                    onFast={setFast}
                />
                <Variants
                    offered={branch === NEW_BRANCH}
                    agents={agents}
                    first={{ agent: agentId, model }}
                    variants={variants}
                    onChange={setVariants}
                />
            </ComposerBox>
            <AgentNotice agentId={agentId} />
            <p className="new-task-note" aria-live="polite">
                {creating !== null && <Incubating />}
                {note(creating, prompt, hookLine, words)}
            </p>
        </div>
    );
}

/** Stands for the task's name while it is still being thought of. */
const NAMING = "";

/** The line under the box: what sending will do, or what is being done. */
function note(creating: string | null, prompt: string, hookLine: string | null, words: Words) {
    if (creating === null) {
        return prompt.trim() !== "" && words.newTaskHint;
    }
    return creating === NAMING
        ? words.namingTask
        : `${words.settingUp(creating)} ${hookLine ?? ""}`;
}

/** What a model calls the task, or `null` to name it after the prompt. Never fails a send. */
async function named(project: string, prompt: string): Promise<TaskNaming | null> {
    return api.taskNameFor(project, prompt).catch(() => null);
}

interface Start {
    project: string;
    name: string;
    /** The first session's title, if the task was named by a model. */
    title: string | null;
    branch: string | null;
    /** The environment of the project to set the task up in, if one was chosen. */
    environment: string | null;
    setup: SessionSetup;
    prompt: string;
    /** The files the first message points the agent at. */
    files: string[];
}

/**
 * Sends the same prompt to every other agent chosen, each from the same base in a task of
 * its own. One that cannot be started is reported; the others carry on. An existing branch
 * can only be checked out once, so there the variants are left out.
 */
async function startVariants(
    start: Start,
    variants: Variant[],
    taken: string[],
    how: { supervision: Supervision; effort: Effort | null },
) {
    const also = start.branch === null ? variants : [];
    const names = variantNames(start.name, also, taken);
    const { report, refresh } = useStore.getState();
    const started = also.map((variant, index) => {
        const setup = setupFor(variant.agent, variant.model, how.supervision, how.effort);
        return startTask({ ...start, name: names[index] ?? start.name, setup }, false).catch(
            report,
        );
    });
    await Promise.all(started);
    if (also.length > 0) {
        await refresh(api);
    }
}

/** Creates the task and its first session, sends the prompt, and opens it if asked to. */
async function startTask(start: Start, show: boolean) {
    const { project, name, title, setup, prompt, files } = start;
    const { refresh, open } = useStore.getState();
    // The core may have stepped past a name that is taken, to `name-2` and so on.
    const task = await api.taskNew(project, name, start.branch, start.environment);
    const session = await api.sessionNew(project, task.name, setup);
    if (title) {
        await api.sessionRename(session.id, title);
    }
    if (show) {
        await refresh(api);
        await open(api, session.id);
    }
    await api.sessionSend(session.id, prompt, files);
}

interface VariantsProps {
    /** Whether more agents can be added at all: only where a new branch is made. */
    offered: boolean;
    agents: AgentInfo[];
    /** The agent and model the task runs on anyway. */
    first: Variant;
    variants: Variant[];
    onChange(variants: Variant[]): void;
}

/**
 * More agents to run the same prompt on: a menu to add one, and what was added, each with a
 * way to take it off again. Only where a new branch is made, since each needs its own.
 */
function Variants({ offered, agents, first, variants, onChange }: VariantsProps) {
    const words = useWords();
    const choices = variantChoices(agents, [first, ...variants]);
    if (!offered) {
        return null;
    }
    return (
        <>
            {choices.length > 0 && (
                <Picker
                    label="Also run on another agent or model"
                    value=""
                    onChange={(key) => key && onChange([...variants, variantFrom(key)])}
                    options={[
                        ["", words.alsoRunOn],
                        ...choices.map((choice): [string, string] => [
                            variantKey(choice),
                            variantLabel(choice, agents),
                        ]),
                    ]}
                />
            )}
            {variants.map((variant) => (
                <button
                    key={variantKey(variant)}
                    type="button"
                    className="variant-chip"
                    title="Take this one off"
                    onClick={() =>
                        onChange(
                            variants.filter((other) => variantKey(other) !== variantKey(variant)),
                        )
                    }
                >
                    + {variantLabel(variant, agents)} ×
                </button>
            ))}
        </>
    );
}

interface AgentPickersProps {
    agents: AgentInfo[];
    agentId: string;
    model: string;
    onAgent(id: string): void;
    onModel(model: string): void;
    effort: Effort | null;
    onEffort(effort: Effort | null): void;
    fast: boolean;
    onFast(fast: boolean): void;
}

/** What the session is created with. An effort only goes to an agent that takes one. */
function setupFor(
    agentId: string,
    model: string,
    supervision: Supervision,
    effort: Effort | null,
    fast = false,
): SessionSetup {
    const claude = agentId === CLAUDE;
    return {
        agent: claude ? null : agentId,
        model: model || null,
        supervision,
        effort: takesEffort(agentId) ? effort : null,
        fast: fast && takesFast(agentId),
    };
}

/** The agent menu, shown only when there is a choice, and the models of the chosen agent. */
function AgentPickers(props: AgentPickersProps) {
    const { agents, agentId, model, onAgent, onModel, effort, onEffort } = props;
    const agent = agents.find((candidate) => candidate.id === agentId);
    const usual = useEffortDefault(agentId, model || null);
    return (
        <>
            {agents.length > 1 && (
                <Picker
                    label="Agent"
                    value={agentId}
                    onChange={onAgent}
                    options={agents.map((candidate) => [candidate.id, candidate.name])}
                />
            )}
            <Picker label="Model" value={model} onChange={onModel} options={modelChoices(agent)} />
            {takesEffort(agentId) && (
                <EffortPicker value={effort} applied={usual} onChange={onEffort} />
            )}
            {takesFast(agentId) && <FastToggle on={props.fast} onChange={props.onFast} />}
        </>
    );
}

/** The branches a task in `project` can start from; `null` until they are read. */
/** The environment picked, as long as the project still has it; `null` for the default setup. */
function chosenEnvironment(names: string[], picked: string): string | null {
    return names.includes(picked) ? picked : null;
}

/** The menu of a project's environments. A project without any has no menu. */
function EnvironmentPicker(props: {
    names: string[];
    value: string;
    onChange(value: string): void;
}) {
    const words = useWords();
    if (props.names.length === 0) {
        return null;
    }
    return (
        <Picker
            label={words.environment}
            value={chosenEnvironment(props.names, props.value) ?? ""}
            onChange={props.onChange}
            options={[
                ["", words.defaultSetup],
                ...props.names.map((name): [string, string] => [name, name]),
            ]}
        />
    );
}

/** The names of the project's environments, read again whenever they may have been edited. */
function useEnvironments(project: string | undefined): string[] {
    const [names, setNames] = useState<string[]>([]);
    // The dialog that edits them closing is when they may have changed.
    const editing = useStore((state) => state.environmentsOf);
    // biome-ignore lint/correctness/useExhaustiveDependencies: editing exists to trigger a re-read
    useEffect(() => {
        setNames([]);
        if (project === undefined) {
            return;
        }
        let current = true;
        api.environments(project)
            .then((read) => current && setNames(Object.keys(read)))
            // Without them a task is simply set up the default way.
            .catch(() => undefined);
        return () => {
            current = false;
        };
    }, [project, editing]);
    return names;
}

function useBranchChoices(project: string | undefined): BranchChoices | null {
    const [choices, setChoices] = useState<BranchChoices | null>(null);
    useEffect(() => {
        setChoices(null);
        if (project === undefined) {
            return;
        }
        let current = true;
        api.branchChoices(project)
            .then((read) => current && setChoices(read))
            .catch(useStore.getState().report);
        return () => {
            current = false;
        };
    }, [project]);
    return choices;
}

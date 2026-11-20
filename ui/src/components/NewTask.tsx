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
import { expandPastes, forgetPastes } from "../pastes";
import { useStore } from "../store";
import { taskNameForPrompt, taskNameFrom, uniqueTaskName } from "../tree";
import { wordsFor } from "../wording";
import { AgentNotice } from "./AgentNotice";
import { ComposerBox } from "./ComposerBox";
import { EffortPicker } from "./EffortPicker";
import { Incubating } from "./Incubating";
import { ModePicker } from "./ModePicker";
import { CLAUDE, modelChoices, useAgents, useSlashCommands } from "./options";
import { Picker } from "./Picker";

/** The value of the branch picker that means "create a new branch". */
const NEW_BRANCH = "";

/** Starts a task from a prompt: pick the repository and branch, type, send. */
export function NewTask({ projects }: { projects: Project[] }) {
    const draftProject = useStore((state) => state.draftProject);
    const hookLine = useStore((state) => state.hookLine);
    const words = useStore((state) => wordsFor(state.settings?.palette));
    const [chosen, setChosen] = useState<string | null>(null);
    const [branch, setBranch] = useState(NEW_BRANCH);
    // The menu starts on the default from the settings and keeps what the user picks.
    const defaultSupervision = useStore((state) => state.settings?.default_supervision);
    const [pickedSupervision, setSupervision] = useState<Supervision | null>(null);
    const supervision = pickedSupervision ?? defaultSupervision ?? "auto";
    const [agentId, setAgentId] = useState(CLAUDE);
    const [model, setModel] = useState("");
    const [effort, setEffort] = useState<Effort | null>(null);
    const agents = useAgents();
    const commands = useSlashCommands(null);
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

    const taken = project.tasks.filter((task) => !task.archived).map((task) => task.name);
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
            await startTask({
                project: project.name,
                name: chosen,
                title: naming?.title ?? null,
                branch: branch === NEW_BRANCH ? null : branch,
                environment: chosenEnvironment(environments, environment),
                setup: setupFor(agentId, model, supervision, effort),
                prompt: expandPastes(prompt.trim()),
                files,
            });
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
                placeholder="Describe the task"
                disabled={creating !== null}
                autoFocus
                commands={commands}
            >
                <Picker
                    label="Repository"
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
                />
            </ComposerBox>
            <AgentNotice agentId={agentId} />
            <p className="new-task-note" aria-live="polite">
                {creating !== null && <Incubating />}
                {note(creating, prompt, hookLine)}
            </p>
        </div>
    );
}

/** Stands for the task's name while it is still being thought of. */
const NAMING = "";

/** The line under the box: what sending will do, or what is being done. */
function note(creating: string | null, prompt: string, hookLine: string | null) {
    if (creating === null) {
        return prompt.trim() !== "" && "Creates a task for this in its own worktree.";
    }
    return creating === NAMING ? "Naming the task…" : `Setting up ${creating}… ${hookLine ?? ""}`;
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

/** Creates the task and its first session, opens it, and sends the prompt. */
async function startTask(start: Start) {
    const { project, name, title, setup, prompt, files } = start;
    const { refresh, open } = useStore.getState();
    await api.taskNew(project, name, start.branch, start.environment);
    const session = await api.sessionNew(project, name, setup);
    if (title) {
        await api.sessionRename(session.id, title);
    }
    await refresh(api);
    await open(api, session.id);
    await api.sessionSend(session.id, prompt, files);
}

interface AgentPickersProps {
    agents: AgentInfo[];
    agentId: string;
    model: string;
    onAgent(id: string): void;
    onModel(model: string): void;
    effort: Effort | null;
    onEffort(effort: Effort | null): void;
}

/** What the session is created with. Only Claude Code takes an effort. */
function setupFor(
    agentId: string,
    model: string,
    supervision: Supervision,
    effort: Effort | null,
): SessionSetup {
    const claude = agentId === CLAUDE;
    return {
        agent: claude ? null : agentId,
        model: model || null,
        supervision,
        effort: claude ? effort : null,
    };
}

/** The agent menu, shown only when there is a choice, and the models of the chosen agent. */
function AgentPickers(props: AgentPickersProps) {
    const { agents, agentId, model, onAgent, onModel, effort, onEffort } = props;
    const agent = agents.find((candidate) => candidate.id === agentId);
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
            {agentId === CLAUDE && <EffortPicker value={effort} onChange={onEffort} />}
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
    if (props.names.length === 0) {
        return null;
    }
    return (
        <Picker
            label="Environment"
            value={chosenEnvironment(props.names, props.value) ?? ""}
            onChange={props.onChange}
            options={[
                ["", "Default setup"],
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

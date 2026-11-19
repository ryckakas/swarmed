import { useEffect, useState } from "react";
import { api } from "../api";
import type { AgentInfo, BranchChoices, Project, SessionSetup, Supervision } from "../api/types";
import { useStore } from "../store";
import { taskNameForPrompt, taskNameFrom, uniqueTaskName } from "../tree";
import { ComposerBox } from "./ComposerBox";
import {
    CLAUDE,
    modelChoices,
    SUPERVISION_CHOICES,
    SUPERVISION_LABELS,
    useAgents,
    useSlashCommands,
} from "./options";
import { Picker } from "./Picker";

/** The value of the branch picker that means "create a new branch". */
const NEW_BRANCH = "";

/** Starts a task from a prompt: pick the repository and branch, type, send. */
export function NewTask({ projects }: { projects: Project[] }) {
    const draftProject = useStore((state) => state.draftProject);
    const hookLine = useStore((state) => state.hookLine);
    const [chosen, setChosen] = useState<string | null>(null);
    const [branch, setBranch] = useState(NEW_BRANCH);
    // The menu starts on the default from the settings and keeps what the user picks.
    const defaultSupervision = useStore((state) => state.settings?.default_supervision);
    const [pickedSupervision, setSupervision] = useState<Supervision | null>(null);
    const supervision = pickedSupervision ?? defaultSupervision ?? "auto";
    const [agentId, setAgentId] = useState(CLAUDE);
    const [model, setModel] = useState("");
    const agents = useAgents();
    const commands = useSlashCommands(null);
    const [prompt, setPrompt] = useState("");
    const [creating, setCreating] = useState<string | null>(null);

    // The sidebar's plus on a project preselects it; otherwise the last choice, or the first.
    const project =
        projects.find((candidate) => candidate.name === (chosen ?? draftProject)) ?? projects[0];
    const choices = useBranchChoices(project?.name);
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
        setCreating(name);
        useStore.setState({ hookLine: null });
        try {
            await startTask({
                project: project.name,
                name,
                branch: branch === NEW_BRANCH ? null : branch,
                setup: {
                    agent: agentId === CLAUDE ? null : agentId,
                    model: model || null,
                    supervision,
                },
                prompt: prompt.trim(),
            });
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
            <h1>What should the agent work on?</h1>
            <ComposerBox
                value={prompt}
                onChange={setPrompt}
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
                <Picker
                    label="Supervision"
                    value={supervision}
                    onChange={(value) => setSupervision(value as Supervision)}
                    options={SUPERVISION_CHOICES.map((choice) => [
                        choice,
                        SUPERVISION_LABELS[choice],
                    ])}
                />
                <AgentPickers
                    agents={agents}
                    agentId={agentId}
                    model={model}
                    onAgent={(value) => {
                        setAgentId(value);
                        setModel("");
                    }}
                    onModel={setModel}
                />
            </ComposerBox>
            <p className="new-task-note" aria-live="polite">
                {creating === null
                    ? prompt.trim() !== "" && `Creates the task ${name} in its own worktree.`
                    : `Setting up ${creating}… ${hookLine ?? ""}`}
            </p>
        </div>
    );
}

interface Start {
    project: string;
    name: string;
    branch: string | null;
    setup: SessionSetup;
    prompt: string;
}

/** Creates the task and its first session, opens it, and sends the prompt. */
async function startTask({ project, name, branch, setup, prompt }: Start) {
    const { refresh, open } = useStore.getState();
    await api.taskNew(project, name, branch);
    const session = await api.sessionNew(project, name, setup);
    await refresh(api);
    await open(api, session.id);
    await api.sessionSend(session.id, prompt);
}

interface AgentPickersProps {
    agents: AgentInfo[];
    agentId: string;
    model: string;
    onAgent(id: string): void;
    onModel(model: string): void;
}

/** The agent menu, shown only when there is a choice, and the models of the chosen agent. */
function AgentPickers({ agents, agentId, model, onAgent, onModel }: AgentPickersProps) {
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
        </>
    );
}

/** The branches a task in `project` can start from; `null` until they are read. */
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

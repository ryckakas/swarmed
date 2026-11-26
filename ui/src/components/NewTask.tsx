import { useEffect, useState } from "react";
import { api } from "../api";
import type {
    AgentInfo,
    BranchChoices,
    Effort,
    EnvironmentChoice,
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
import { branchAsked, newTaskPlace, newTaskSays, repositoryChoices } from "../chats";
import {
    environmentChoice,
    environmentOptions,
    environmentShown,
    OWN_SETUP,
} from "../environments";
import { githubOf } from "../githubMentions";
import { mentionedIn, mentionsAsWords } from "../mentions";
import { expandTokens, forgetTokens } from "../pastes";
import { removedWhileSetUp } from "../removal";
import { githubRead, NO_DRAFT } from "../sending";
import { sentOn } from "../slash";
import { hookKey, useStore } from "../store";
import { taskNameForPrompt, taskNameFrom, uniqueTaskName } from "../tree";
import { useWords } from "../useWords";
import type { Words } from "../wording";
import { capitalized, wordsFor } from "../wording";
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
import { RoleMenu } from "./Role";

/** The value of the branch picker that means "create a new branch". */
const NEW_BRANCH = "";

/**
 * Starts a task from a prompt: pick the repository and branch, type, send. With no repository,
 * which is where it starts unless opened for one, it starts a chat.
 */
export function NewTask({ projects }: { projects: Project[] }) {
    const draftProject = useStore((state) => state.draftProject);
    const words = useStore((state) => wordsFor(state.settings?.style));
    const [chosen, setChosen] = useState<string | null>(null);
    const [branch, setBranch] = useState(NEW_BRANCH);
    // The view stays when it is opened again, for another repository or for none: what was
    // picked for the last one must not stand in the way of this one, nor its branch be sent.
    const [openedFor, setOpenedFor] = useState(draftProject);
    if (openedFor !== draftProject) {
        setOpenedFor(draftProject);
        setChosen(null);
        setBranch(NEW_BRANCH);
    }
    // The menu starts on the default from the settings and keeps what the user picks.
    const defaultSupervision = useStore((state) => state.settings?.default_supervision);
    const [pickedSupervision, setSupervision] = useState<Supervision | null>(null);
    const supervision = pickedSupervision ?? defaultSupervision ?? "auto";
    const [agentId, setAgentId] = useState(CLAUDE);
    const [model, setModel] = useState("");
    const [effort, setEffort] = useState<Effort | null>(null);
    const [fast, setFast] = useState(false);
    const [role, setRole] = useState<string | null>(null);
    // More agents to run the same prompt on, each in a task of its own.
    const [variants, setVariants] = useState<Variant[]>([]);
    const agents = useAgents();
    // A task starts with a message to its agent: a screen or a setting has no session yet.
    const commands = sentOn(useSlashCommands(null));

    // The sidebar's plus on a repository preselects it; otherwise the last choice, or none,
    // for a chat.
    const { repository, home, taken } = newTaskPlace(projects, chosen, draftProject);
    const { creating, hookLine } = useCreating(home);
    const noteCreating = useStore((state) => state.noteCreating);
    const says = newTaskSays(words, repository);
    // What was typed for this repository and not sent, apart from every other repository's.
    const draft = useStore((state) => state.newTaskDrafts[home] ?? NO_DRAFT);
    const { text: prompt, files, images } = draft;
    const editNewTaskDraft = useStore((state) => state.editNewTaskDraft);
    // The issue the task is started for, if it was started from one: the task records it.
    const issue = useStore((state) => state.newTaskIssues[home] ?? null);
    const linkIssue = useStore((state) => state.linkIssue);
    const choices = useBranchChoices(repository?.name);
    const environments = useEnvironments(repository?.name);
    // What was picked, by repository: one where nothing was picked starts on its default.
    const [picked, setPicked] = useState<Record<string, string>>({});
    // Nothing is ever picked for the chats, which have no environments.
    const environment = picked[home];

    // A removed task's name too, since its branch may still be there. The core has the last
    // word: it steps past branches here and on a remote, and the task it makes says its name.
    const from = branchAsked(repository, branch, NEW_BRANCH);
    // A session the prompt mentions is named by its title's words, not by its token.
    const base = from === null ? taskNameForPrompt(mentionsAsWords(prompt)) : taskNameFrom(from);
    const name = uniqueTaskName(base || "task", taken);
    const canSend = prompt.trim() !== "" && creating === null;

    const send = async () => {
        if (!canSend) {
            return;
        }
        noteCreating(home, NAMING);
        try {
            // A task on a new branch is named for what it is about; one on an existing branch
            // keeps that branch's name.
            const asked = mentionsAsWords(prompt.trim());
            const naming = from === null ? await named(home, asked) : null;
            const chosen = naming ? uniqueTaskName(naming.name, taken) : name;
            useStore.getState().forgetHookLine(hookKey(home, chosen));
            noteCreating(home, chosen);
            const start = {
                project: home,
                name: chosen,
                title: naming?.title ?? null,
                branch: from,
                environment: environmentChoice(environments.names, environment),
                setup: { ...setupFor(agentId, model, supervision, effort, fast), role },
                prompt: expandTokens(prompt.trim()),
                files,
                sessions: mentionedIn(prompt),
                github: githubOf(prompt, repository?.name),
                // Kept by the core as the session's own, once there is a session.
                images: images.map((image) => image.data),
                issue: issue?.number ?? null,
            };
            // The pull requests and issues it names are read first: the core lists them from that.
            await githubRead(api, home, start.github);
            await startTask(start, true);
            // It is a task now, with the prompt waiting in its session; one that could not be
            // made leaves the prompt here to try again.
            editNewTaskDraft(start.project, NO_DRAFT);
            linkIssue(start.project, null);
            noteCreating(home, null);
            await startVariants(start, variants, taken, { supervision, effort, role });
            forgetTokens(prompt);
        } catch (error) {
            reportStartFailed(error);
            await useStore.getState().refresh(api);
            noteCreating(home, null);
        }
    };

    return (
        <div className="new-task">
            <h1>{words.newTask}</h1>
            <ComposerBox
                value={prompt}
                onChange={(text) => editNewTaskDraft(home, { text })}
                files={files}
                onFiles={(added) => editNewTaskDraft(home, { files: added })}
                images={images}
                onImages={(added) => editNewTaskDraft(home, { images: added })}
                onSend={() => void send()}
                canSend={canSend}
                placeholder={says.placeholder}
                disabled={creating !== null}
                autoFocus
                commands={commands}
                mentionFrom={{ project: home }}
                githubFrom={repository?.name}
            >
                {issue && (
                    <button
                        type="button"
                        className="variant-chip issue-chip"
                        title={`Started for issue #${issue.number}: ${issue.title}. Press to start it for no issue.`}
                        onClick={() => linkIssue(home, null)}
                    >
                        For issue #{issue.number} ×
                    </button>
                )}
                <Picker
                    label={words.repository}
                    value={home}
                    onChange={(value) => {
                        setChosen(value);
                        setBranch(NEW_BRANCH);
                    }}
                    options={repositoryChoices(projects, words.noRepository)}
                />
                {repository && (
                    <RepositoryPickers
                        branch={branch}
                        onBranch={setBranch}
                        choices={choices}
                        environments={environments}
                        environment={environment}
                        onEnvironment={(name) => setPicked({ ...picked, [repository.name]: name })}
                    />
                )}
                <ModePicker value={supervision} agentId={agentId} onChange={setSupervision} />
                <RoleMenu value={role} onChange={setRole} />
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
                {note(creating, prompt, hookLine, says.hint, words)}
            </p>
        </div>
    );
}

interface RepositoryPickersProps {
    branch: string;
    onBranch(branch: string): void;
    choices: BranchChoices | null;
    environments: Environments;
    environment: string | undefined;
    onEnvironment(name: string): void;
}

/** What only a task in a repository is asked: the branch it starts from, and its environment. */
function RepositoryPickers(props: RepositoryPickersProps) {
    const words = useWords();
    const { choices } = props;
    return (
        <>
            <Picker
                label={capitalized(words.terms.branch)}
                value={props.branch}
                onChange={props.onBranch}
                options={[
                    [
                        NEW_BRANCH,
                        `New ${words.terms.branch} from ${choices?.base ?? `the default ${words.terms.branch}`}`,
                    ],
                    ...(choices?.existing ?? []).map((existing): [string, string] => [
                        existing,
                        existing,
                    ]),
                ]}
            />
            <EnvironmentPicker
                environments={props.environments}
                picked={props.environment}
                onChange={props.onEnvironment}
            />
        </>
    );
}

/**
 * The task the new-task view of repository `home` is making, if any, and what making it said
 * last. Kept in the store, by repository: going elsewhere while a task is made and coming back
 * finds it still being made.
 */
function useCreating(home: string): { creating: string | null; hookLine: string | null } {
    const creating = useStore((state) => state.creating[home]);
    const hookLine = useStore((state) =>
        creating === undefined ? undefined : state.hookLines[hookKey(home, creating)],
    );
    return { creating: creating ?? null, hookLine: hookLine ?? null };
}

/** Stands for the task's name while it is still being thought of. */
const NAMING = "";

/** The line under the box: what sending will do, `hint`, or what is being done. */
function note(
    creating: string | null,
    prompt: string,
    hookLine: string | null,
    hint: string,
    words: Words,
) {
    if (creating === null) {
        return prompt.trim() !== "" && hint;
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
    /** The environment of the project to set the task up in. */
    environment: EnvironmentChoice;
    setup: SessionSetup;
    prompt: string;
    /** The files the first message points the agent at. */
    files: string[];
    /** The sessions the first message mentions. */
    sessions: number[];
    /** The pull requests and issues the first message mentions, by number. */
    github: number[];
    /** Images pasted into the first message, in base64. */
    images: string[];
    /** The number of the repository's issue the task is for, if any. */
    issue: number | null;
}

/**
 * Sends the same prompt to every other agent chosen, each from the same base in a task of
 * its own, or each in a chat of its own: each set up beside the others, and shown in the
 * sidebar as it is. One that cannot be started is reported; the others carry on. An existing
 * branch can only be checked out once, so there the variants are left out.
 */
async function startVariants(
    start: Start,
    variants: Variant[],
    taken: string[],
    how: { supervision: Supervision; effort: Effort | null; role: string | null },
) {
    const also = start.branch === null ? variants : [];
    const names = variantNames(start.name, also, taken);
    const started = also.map((variant, index) => {
        const chosen = setupFor(variant.agent, variant.model, how.supervision, how.effort);
        // Each does the same work, so each is for the same.
        const setup = { ...chosen, role: how.role };
        return startTask({ ...start, name: names[index] ?? start.name, setup }, false).catch(
            reportStartFailed,
        );
    });
    await Promise.all(started);
    if (also.length > 0) {
        await useStore.getState().refresh(api);
    }
}

/**
 * Says why a task could not be started, unless it was removed while it was set up: the user
 * did that on purpose, and the prompt is still in the box.
 */
function reportStartFailed(error: unknown) {
    if (!removedWhileSetUp(error)) {
        useStore.getState().report(error);
    }
}

/**
 * Starts the task and its first session, which the core makes at once with the prompt waiting
 * for the task's setup, and opens it if asked to: the session shows the setup as it goes, and
 * the prompt goes once it is done. The core may have stepped past a name that is taken, to
 * `name-2` and so on.
 */
async function startTask(start: Start, show: boolean) {
    const { project, name, title, setup, prompt, files, sessions, github, images, issue } = start;
    const { refresh, open } = useStore.getState();
    const environment = start.environment;
    const how = { branch: start.branch, environment, setup, title };
    const begun = { ...how, prompt, files, sessions, github, images, issue };
    const { session } = await api.taskStart(project, name, begun);
    await refresh(api);
    if (show) {
        await open(api, session.id);
    }
}

interface VariantsProps {
    /** Whether more agents can be added at all: only where a new branch is made, or a chat. */
    offered: boolean;
    agents: AgentInfo[];
    /** The agent and model the task runs on anyway. */
    first: Variant;
    variants: Variant[];
    onChange(variants: Variant[]): void;
}

/**
 * More agents to run the same prompt on: a menu to add one, and what was added, each with a
 * way to take it off again. Only where a new branch is made, since each needs its own, or in a
 * chat, where each is a chat of its own.
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
                    label={`Also run on another ${words.terms.agent} or ${words.terms.model}`}
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
    const words = useWords();
    const { agents, agentId, model, onAgent, onModel, effort, onEffort } = props;
    const agent = agents.find((candidate) => candidate.id === agentId);
    const usual = useEffortDefault(agentId, model || null);
    return (
        <>
            {agents.length > 1 && (
                <Picker
                    label={capitalized(words.terms.agent)}
                    value={agentId}
                    onChange={onAgent}
                    options={agents.map((candidate) => [candidate.id, candidate.name])}
                />
            )}
            <Picker
                label={capitalized(words.terms.model)}
                value={model}
                onChange={onModel}
                options={modelChoices(agent, null, words)}
            />
            {takesEffort(agentId) && (
                <EffortPicker value={effort} applied={usual} onChange={onEffort} />
            )}
            {takesFast(agentId) && <FastToggle on={props.fast} onChange={props.onFast} />}
        </>
    );
}

/** The branches a task in `project` can start from; `null` until they are read. */
/** A project's environments by name, and the one new tasks are set up in unless told otherwise. */
interface Environments {
    names: string[];
    usual: string | null;
}

const NO_ENVIRONMENTS: Environments = { names: [], usual: null };

/**
 * The menu of a project's environments, on its default until another is picked. A project
 * without any has no menu.
 */
function EnvironmentPicker(props: {
    environments: Environments;
    picked: string | undefined;
    onChange(value: string): void;
}) {
    const words = useWords();
    const { names, usual } = props.environments;
    if (names.length === 0) {
        return null;
    }
    return (
        <Picker
            label={words.environment}
            value={environmentShown(names, usual, props.picked)}
            onChange={props.onChange}
            options={[[OWN_SETUP, words.ownSetup], ...environmentOptions(names, usual)]}
        />
    );
}

/** The project's environments and its default, read again whenever they may have been edited. */
function useEnvironments(project: string | undefined): Environments {
    const [environments, setEnvironments] = useState(NO_ENVIRONMENTS);
    // The dialog that edits them closing is when they may have changed.
    const editing = useStore((state) => state.environmentsOf);
    // biome-ignore lint/correctness/useExhaustiveDependencies: editing exists to trigger a re-read
    useEffect(() => {
        setEnvironments(NO_ENVIRONMENTS);
        if (project === undefined) {
            return;
        }
        let current = true;
        Promise.all([api.environments(project), api.environmentDefault(project)])
            .then(([read, usual]) => {
                if (current) {
                    setEnvironments({ names: Object.keys(read), usual });
                }
            })
            // Without them the menu is left out, and the core still sets the task up in the
            // project's default.
            .catch(() => undefined);
        return () => {
            current = false;
        };
    }, [project, editing]);
    return environments;
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

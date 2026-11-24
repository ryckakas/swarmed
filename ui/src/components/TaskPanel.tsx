import { useState } from "react";
import { api } from "../api";
import type { Project, Task } from "../api/types";
import { PANEL_WIDTH } from "../resize";
import { useStore } from "../store";
import { findSession, findSessionHome } from "../tree";
import { useWords } from "../useWords";
import { capitalized } from "../wording";
import { ActionBar } from "./ActionBar";
import { Background, runningCount, useBackground } from "./Background";
import { Changes } from "./Changes";
import { Files } from "./Files";
import { CloseIcon, EditorIcon, IconButton, TerminalIcon } from "./IconButton";
import { IssuePart } from "./IssuePart";
import { editorName } from "./options";
import { Picker } from "./Picker";
import { Publish } from "./Publish";
import { PullSection } from "./PullRequest";
import { PutAwayPart } from "./PutAway";
import { Resizer } from "./Resizer";
import { StackControls } from "./StackControls";

interface Props {
    width: number;
    onResize(width: number): void;
}

/** The right-hand side of the window: the selected session's task, and ways out of Swarmed. */
export function TaskPanel({ width, onResize }: Props) {
    const words = useWords();
    const projects = useStore((state) => state.projects);
    const selectedSessionId = useStore((state) => state.selectedSessionId);
    const open = useStore((state) => state.panelOpen);
    const session = findSession(projects, selectedSessionId);
    const home = session && findSessionHome(projects, session);
    if (!(open && home)) {
        return null;
    }
    return (
        <>
            <Resizer
                label={`Resize ${words.terms.task} panel`}
                width={width}
                limits={PANEL_WIDTH}
                side="right"
                onResize={onResize}
            />
            <aside
                className="task-panel"
                aria-label={`${capitalized(words.terms.task)} ${home.task.name}`}
            >
                <div className="drag-strip" data-tauri-drag-region />
                {/* Each task its own, so what one is doing or was typed for it stays with it:
                    a stack stopping, an open log, a commit message. */}
                <TaskHead key={`head-${home.task.id}`} project={home.project} task={home.task} />
                <Tabs key={home.task.id} project={home.project} task={home.task} />
            </aside>
        </>
    );
}

type Tab = "files" | "changes" | "background";

/**
 * The lower part of the panel, one tab at a time: the task's files, what the task changed, and
 * what its agents left running. The last tab says how many of those still run. A chat has no
 * repository, and so no changes: its files come first.
 */
function Tabs({ project, task }: { project: Project; task: Task }) {
    const words = useWords();
    const [tab, setTab] = useState<Tab>(project.chats ? "files" : "changes");
    const { tasks, read } = useBackground(project.name, task.name);
    const running = runningCount(tasks);
    const changes: [Tab, string][] = project.chats ? [] : [["changes", words.changes]];
    const tabs: [Tab, string][] = [
        ["files", words.files],
        ...changes,
        ["background", running > 0 ? `${words.background} (${running})` : words.background],
    ];
    return (
        <>
            <div
                className="panel-tabs"
                role="tablist"
                aria-label={`What to show of the ${words.terms.task}`}
            >
                {tabs.map(([id, label]) => (
                    <button
                        key={id}
                        type="button"
                        role="tab"
                        className="panel-tab"
                        aria-selected={tab === id}
                        onClick={() => setTab(id)}
                    >
                        {label}
                    </button>
                ))}
            </div>
            {tab === "files" && <Files project={project} task={task} />}
            {tab === "changes" && <Changes project={project} task={task} />}
            {tab === "background" && (
                <section className="changes" aria-label={words.background}>
                    <div className="changes-scroll">
                        <Background tasks={tasks} onChanged={() => void read()} />
                    </div>
                </section>
            )}
        </>
    );
}

/**
 * What the task is, and the buttons that open it elsewhere. The terminal and the editor are the
 * ones chosen in the settings, so their buttons are icons beside the one that hides the panel.
 */
function TaskHead({ project, task }: { project: Project; task: Task }) {
    const words = useWords();
    const report = useStore((state) => state.report);
    const editor = useStore((state) => state.settings?.editor ?? "code");
    const terminalApp = useStore((state) => state.settings?.terminal_app ?? "Terminal");
    const togglePanel = useStore((state) => state.togglePanel);
    return (
        <header className="panel-head">
            <div className="panel-tools">
                <IconButton
                    label={`Open in ${terminalApp}`}
                    onClick={() => api.taskOpenTerminal(project.name, task.name).catch(report)}
                >
                    <TerminalIcon />
                </IconButton>
                <IconButton
                    label={`Open in ${editorName(editor)}`}
                    onClick={() => api.taskOpenEditor(project.name, task.name).catch(report)}
                >
                    <EditorIcon />
                </IconButton>
                <IconButton label={`Hide the ${words.terms.task} panel`} onClick={togglePanel}>
                    <CloseIcon />
                </IconButton>
            </div>
            <h2>{task.name}</h2>
            <p className="mono panel-branch" title={task.worktree_path}>
                {project.chats ? words.noRepository : task.branch}
            </p>
            {task.urls.length > 0 && (
                <div className="panel-actions">
                    {task.urls.map((url) => (
                        <button
                            key={url}
                            type="button"
                            className="button"
                            title={`Opens ${url}`}
                            onClick={() => api.openLink(url).catch(report)}
                        >
                            {browserLabel(url, task.urls.length)}
                        </button>
                    ))}
                </div>
            )}
            <ActionBar project={project.name} />
            {!project.chats && <RepositoryParts project={project} task={task} />}
        </header>
    );
}

/**
 * What only a task in a repository has: the issue it is for, its stack, comparing, publishing,
 * its pull request, and putting it away.
 */
function RepositoryParts({ project, task }: { project: Project; task: Task }) {
    return (
        <>
            <IssuePart project={project.name} task={task} />
            <StackControls project={project.name} task={task} />
            <CompareWith project={project} task={task} />
            <Publish project={project.name} task={task} />
            <PullSection project={project.name} task={task} />
            <PutAwayPart project={project.name} task={task} />
        </>
    );
}

/** What the button that opens a stack's address says: which address, when there are several. */
export function browserLabel(url: string, of: number): string {
    return of > 1
        ? `Open ${url.replace(/^https?:\/\//, "").replace(/\/$/, "")}`
        : "Open in browser";
}

/** Sets this task beside another of the same repository. Nothing without another task. */
function CompareWith({ project, task }: { project: Project; task: Task }) {
    const words = useWords();
    const compare = useStore((state) => state.compare);
    const others = project.tasks.filter((other) => !other.archived && other.id !== task.id);
    if (others.length === 0) {
        return null;
    }
    return (
        <div className="panel-actions">
            <Picker
                label={words.compareTask}
                value=""
                onChange={(other) =>
                    other && compare({ project: project.name, left: task.name, right: other })
                }
                options={[
                    ["", "Compare with…"],
                    ...others.map((other): [string, string] => [other.name, other.name]),
                ]}
            />
        </div>
    );
}

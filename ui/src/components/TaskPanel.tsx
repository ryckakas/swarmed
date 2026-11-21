import { useState } from "react";
import { api } from "../api";
import type { Project, Task } from "../api/types";
import { PANEL_WIDTH } from "../resize";
import { useStore } from "../store";
import { findSession, findSessionHome } from "../tree";
import { useWords } from "../useWords";
import { ActionBar } from "./ActionBar";
import { Background, runningCount, useBackground } from "./Background";
import { Changes } from "./Changes";
import { Files } from "./Files";
import { CloseIcon, EditorIcon, IconButton, TerminalIcon } from "./IconButton";
import { editorName } from "./options";
import { Picker } from "./Picker";
import { Publish } from "./Publish";
import { PullSection } from "./PullRequest";
import { Resizer } from "./Resizer";
import { StackControls } from "./StackControls";

interface Props {
    width: number;
    onResize(width: number): void;
}

/** The right-hand side of the window: the selected session's task, and ways out of Swarmed. */
export function TaskPanel({ width, onResize }: Props) {
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
                label="Resize task panel"
                width={width}
                limits={PANEL_WIDTH}
                side="right"
                onResize={onResize}
            />
            <aside className="task-panel" aria-label={`Task ${home.task.name}`}>
                <div className="drag-strip" data-tauri-drag-region />
                <TaskHead project={home.project} task={home.task} />
                <Tabs key={home.task.id} project={home.project} task={home.task} />
            </aside>
        </>
    );
}

type Tab = "files" | "changes" | "background";

/**
 * The lower part of the panel, one tab at a time: the task's files, what the task changed, and
 * what its agents left running. The last tab says how many of those still run.
 */
function Tabs({ project, task }: { project: Project; task: Task }) {
    const words = useWords();
    const [tab, setTab] = useState<Tab>("changes");
    const { tasks, read } = useBackground(project.name, task.name);
    const running = runningCount(tasks);
    const tabs: [Tab, string][] = [
        ["files", words.files],
        ["changes", words.changes],
        ["background", running > 0 ? `${words.background} (${running})` : words.background],
    ];
    return (
        <>
            <div className="panel-tabs" role="tablist" aria-label="What to show of the task">
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
                <IconButton label="Hide the task panel" onClick={togglePanel}>
                    <CloseIcon />
                </IconButton>
            </div>
            <h2>{task.name}</h2>
            <p className="mono panel-branch" title={task.worktree_path}>
                {task.branch}
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
            <StackControls project={project.name} task={task} />
            <CompareWith project={project} task={task} />
            <Publish project={project.name} task={task} />
            <PullSection project={project.name} task={task} />
        </header>
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
    const compare = useStore((state) => state.compare);
    const others = project.tasks.filter((other) => !other.archived && other.id !== task.id);
    if (others.length === 0) {
        return null;
    }
    return (
        <div className="panel-actions">
            <Picker
                label="Compare with another task"
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

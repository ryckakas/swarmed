import { api } from "../api";
import type { Project, Task } from "../api/types";
import { PANEL_WIDTH } from "../resize";
import { useStore } from "../store";
import { findSession, findSessionHome } from "../tree";
import { Changes } from "./Changes";
import { CloseIcon, IconButton } from "./IconButton";
import { editorChoices, editorName } from "./options";
import { Picker } from "./Picker";
import { Resizer } from "./Resizer";

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
                <Changes key={home.task.id} project={home.project} task={home.task} />
            </aside>
        </>
    );
}

/** What the task is, and the buttons that open it elsewhere. */
function TaskHead({ project, task }: { project: Project; task: Task }) {
    const report = useStore((state) => state.report);
    const editor = useStore((state) => state.settings?.editor ?? "code");
    const changeSettings = useStore((state) => state.changeSettings);
    const togglePanel = useStore((state) => state.togglePanel);
    return (
        <header className="panel-head">
            <div className="panel-close">
                <IconButton label="Hide the task panel" onClick={togglePanel}>
                    <CloseIcon />
                </IconButton>
            </div>
            <h2>{task.name}</h2>
            <p className="mono panel-branch" title={task.worktree_path}>
                {task.branch}
            </p>
            <div className="panel-actions">
                <button
                    type="button"
                    className="button"
                    title={`Opens ${task.worktree_path}`}
                    onClick={() => api.taskOpenEditor(project.name, task.name).catch(report)}
                >
                    Open in {editorName(editor)}
                </button>
                <Picker
                    label="Editor"
                    value={editor}
                    onChange={(editor) => void changeSettings(api, { editor })}
                    options={editorChoices(editor)}
                />
                {task.url !== null && (
                    <button
                        type="button"
                        className="button"
                        title={`Opens ${task.url}`}
                        onClick={() => api.taskOpenUrl(project.name, task.name).catch(report)}
                    >
                        Open in browser
                    </button>
                )}
            </div>
        </header>
    );
}

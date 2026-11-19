import { type CSSProperties, useEffect } from "react";
import { api } from "./api";
import { applyAppearance } from "./appearance";
import icon from "./assets/icon.jpg";
import { CloseSessionDialog } from "./components/CloseSessionDialog";
import { NoteDialog } from "./components/NoteDialog";
import { RemoveDialog } from "./components/RemoveDialog";
import { Resizer, useStoredWidth } from "./components/Resizer";
import { SessionPane } from "./components/SessionPane";
import { SettingsDialog } from "./components/SettingsDialog";
import { Sidebar } from "./components/Sidebar";
import { TaskPanel } from "./components/TaskPanel";
import { PANEL_WIDTH, SIDEBAR_WIDTH, widestPanel } from "./resize";
import { connect, useStore } from "./store";
import { useHotkeys } from "./useHotkeys";

export function App() {
    useEffect(() => connect(api), []);
    useHotkeys();
    const load = useStore((state) => state.load);
    const settings = useStore((state) => state.settings);
    useEffect(() => applyAppearance(document.documentElement, settings), [settings]);
    const problem = useStore((state) => state.problem);
    const dismissProblem = useStore((state) => state.dismissProblem);
    const [sidebarWidth, setSidebarWidth] = useStoredWidth("swarmed.sidebar-width", SIDEBAR_WIDTH);
    const [panelWidth, setPanelWidth] = useStoredWidth("swarmed.panel-width", PANEL_WIDTH);
    const widths = {
        "--sidebar-width": `${sidebarWidth}px`,
        "--panel-width": `${panelWidth}px`,
    } as CSSProperties;

    return (
        <div className="app" style={widths}>
            <Sidebar />
            <Resizer
                label="Resize sidebar"
                width={sidebarWidth}
                limits={SIDEBAR_WIDTH}
                side="left"
                onResize={setSidebarWidth}
            />
            <main className="main">
                {/* The window has no title bar; this strip is what the user drags it by. */}
                <div className="drag-strip" data-tauri-drag-region />
                {problem && (
                    <div className="problem" role="alert">
                        <span>{problem}</span>
                        <button type="button" className="button" onClick={dismissProblem}>
                            Dismiss
                        </button>
                    </div>
                )}
                {load === "loading" && <Splash />}
                {load === "ready" && <SessionPane />}
                {typeof load === "object" && <LoadError message={load.error} />}
            </main>
            <TaskPanel
                width={Math.min(panelWidth, widestPanel(window.innerWidth, sidebarWidth))}
                // Stop where the panel stops growing, so dragging back takes effect at once.
                onResize={(width) =>
                    setPanelWidth(Math.min(width, widestPanel(window.innerWidth, sidebarWidth)))
                }
            />
            <CloseSessionDialog />
            <NoteDialog />
            <RemoveDialog />
            <SettingsDialog />
        </div>
    );
}

/** What the window shows for the moment it takes to read the projects. */
function Splash() {
    return (
        <div className="splash">
            <img src={icon} alt="" width={128} height={128} />
            <p>Swarmed</p>
        </div>
    );
}

function LoadError({ message }: { message: string }) {
    const refresh = useStore((state) => state.refresh);
    return (
        <div className="notice" role="alert">
            <h1>Swarmed could not read its projects</h1>
            <p className="mono">{message}</p>
            <button type="button" className="button" onClick={() => void refresh(api)}>
                Try again
            </button>
        </div>
    );
}

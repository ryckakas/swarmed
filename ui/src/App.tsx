import { type CSSProperties, useEffect } from "react";
import { api } from "./api";
import { applyAppearance, customCss, drawCustom } from "./appearance";
import icon from "./assets/icon.jpg";
import { BROWSER_WIDTH } from "./browser";
import { BannerDialog } from "./components/BannerDialog";
import { BrowserColumn } from "./components/BrowserPane";
import { CloseSessionDialog } from "./components/CloseSessionDialog";
import { CloudDialog } from "./components/CloudDialog";
import { CloudDiffDialog } from "./components/CloudDiffDialog";
import { CompareDialog } from "./components/CompareDialog";
import { EnvironmentsDialog } from "./components/EnvironmentsDialog";
import { InboxDialog } from "./components/InboxDialog";
import { IssuesDialog } from "./components/IssuesDialog";
import { NoteDialog } from "./components/NoteDialog";
import { RemoveDialog } from "./components/RemoveDialog";
import { Resizer, useStoredWidth } from "./components/Resizer";
import { RewindDialog } from "./components/RewindDialog";
import { SearchDialog } from "./components/SearchDialog";
import { SessionPane } from "./components/SessionPane";
import { SettingsDialog } from "./components/SettingsDialog";
import { Sidebar } from "./components/Sidebar";
import { TaskPanel } from "./components/TaskPanel";
import { UsageNotice } from "./components/UsageNotice";
import { drawnLayout } from "./customLayout";
import { PANEL_WIDTH, sidebarOf, widestPanel } from "./resize";
import { connect, useStore } from "./store";
import { countUnread, countWaiting } from "./tree";
import { useHotkeys } from "./useHotkeys";

export function App() {
    useEffect(() => connect(api), []);
    useHotkeys();
    const load = useStore((state) => state.load);
    const settings = useStore((state) => state.settings);
    const customLayout = useStore((state) => state.customLayout);
    useEffect(() => {
        applyAppearance(document.documentElement, settings, customLayout);
        drawCustom(document, customCss(settings, customLayout));
    }, [settings, customLayout]);
    // The app's icon counts what waits for the user: questions, and answers not looked at.
    const waiting = useStore((state) => countWaiting(state.projects) + countUnread(state.projects));
    useEffect(() => void api.setBadge(waiting).catch(() => {}), [waiting]);
    const problem = useStore((state) => state.problem);
    const dismissProblem = useStore((state) => state.dismissProblem);
    const sidebar = sidebarOf(drawnLayout(settings, customLayout));
    const [sidebarWidth, setSidebarWidth] = useStoredWidth(sidebar.key, sidebar.limits);
    const [panelWidth, setPanelWidth] = useStoredWidth("swarmed.panel-width", PANEL_WIDTH);
    const [browserShown, setBrowserWidth] = useStoredWidth("swarmed.browser-width", BROWSER_WIDTH);
    const browserWidth = { "--browser-width": `${browserShown}px` } as CSSProperties;
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
                limits={sidebar.limits}
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
                {load === "ready" && <UsageNotice />}
                {load === "loading" && <Splash />}
                {load === "ready" && (
                    <div className="main-body" style={browserWidth}>
                        <div className="main-column">
                            <SessionPane />
                        </div>
                        <BrowserColumn width={browserShown} onResize={setBrowserWidth} />
                    </div>
                )}
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
            <RewindDialog />
            <InboxDialog />
            <CompareDialog />
            <CloudDiffDialog />
            <SearchDialog />
            <NoteDialog />
            <EnvironmentsDialog />
            <IssuesDialog />
            <CloudDialog />
            <RemoveDialog />
            <SettingsDialog />
            <BannerDialog />
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

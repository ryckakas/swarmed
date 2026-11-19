import { useEffect } from "react";
import { api } from "./api";
import { SessionPane } from "./components/SessionPane";
import { Sidebar } from "./components/Sidebar";
import { connect, useStore } from "./store";

export function App() {
    useEffect(() => connect(api), []);
    const load = useStore((state) => state.load);
    const problem = useStore((state) => state.problem);
    const dismissProblem = useStore((state) => state.dismissProblem);

    return (
        <div className="app">
            <Sidebar />
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
                {typeof load === "object" ? <LoadError message={load.error} /> : <SessionPane />}
            </main>
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

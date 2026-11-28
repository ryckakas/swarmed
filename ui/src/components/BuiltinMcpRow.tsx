import { type ReactNode, useEffect, useState } from "react";
import { api } from "../api";
import type {
    BuiltinMcpView,
    BuiltinRegistration,
    DoctorCheck,
    ExistingSetup,
    HeldSession,
} from "../api/types";
import { problemOf } from "../branchPrefix";
import {
    actionLabel,
    type BuiltinAction,
    type BuiltinStep,
    busyLine,
    checksLine,
    checkTone,
    existingLine,
    failedLine,
    heldLine,
    hookNoted,
    isMain,
    offered,
    offersCopy,
    oldServerLine,
    printsOutput,
    registrationPlace,
    registrationStep,
    restartOffered,
    shownFix,
    startsFresh,
    stateLabel,
    stateTone,
    statusMark,
    summaryLine,
    trashOffered,
    unpickedLine,
} from "../builtinMcp";
import { hookKey, useStore } from "../store";
import { useWords } from "../useWords";
import { Dialog } from "./Dialog";

/**
 * The servers Swarmed installs and runs itself, such as repolore, at the top of the MCP
 * servers. Read when the tab opens and after each step the user takes, never on a timer:
 * reading runs the server's own quick doctor. Each step shows what it runs before it does.
 *
 * Folded, as each agent's own servers are. Folded, the heading says each one's state, and a dot
 * pulses on it while a step runs inside, so folding hides no running work; unfolded, the rows
 * say it. The rows stay mounted while folded, so the reading is done when the tab opens.
 */
export function BuiltinMcpSection() {
    const [views, setViews] = useState<BuiltinMcpView[] | null>(null);
    const [problem, setProblem] = useState<string | null>(null);
    useEffect(() => {
        api.builtinMcpServers()
            .then(setViews)
            .catch((error: unknown) => setProblem(problemOf(error)));
    }, []);
    const replace = (changed: BuiltinMcpView) =>
        setViews((all) => (all ?? []).map((view) => (view.name === changed.name ? changed : view)));
    return (
        <details className="builtin-mcp">
            <summary className="builtin-mcp-head">
                <span className="mcp-group-fold" aria-hidden="true">
                    ›
                </span>
                <span className="mcp-group-name">Swarmed built-in</span>
                {problem !== null && (
                    <span className="builtin-mcp-state mcp-status mcp-status-off">not read</span>
                )}
                {(views ?? []).map((view) => (
                    <span key={view.name} className="builtin-mcp-state">
                        {view.name} <StateMark view={view} />
                    </span>
                ))}
            </summary>
            <div className="builtin-mcp-body">
                <p className="settings-hint">
                    Servers Swarmed installs, keeps in a folder of its own and gives to every
                    session. Each is off until you turn it on.
                </p>
                {problem !== null && (
                    <p className="builtin-problem" role="alert">
                        {problem}
                    </p>
                )}
                {views === null && problem === null && <BusyLine>Reading…</BusyLine>}
                {(views ?? []).map((view) => (
                    <BuiltinMcpRow key={view.name} view={view} onChange={replace} />
                ))}
            </div>
        </details>
    );
}

/** What the row is doing: a step, and for a removal, which registration it takes out. */
interface Running {
    step: BuiltinStep;
    key: string | null;
}

interface RowProps {
    view: BuiltinMcpView;
    onChange(view: BuiltinMcpView): void;
}

/**
 * One built-in: its state, what it offers next, and what that state holds. A re-check keeps
 * the figures shown as they are, in full colour, with a line saying it checks.
 */
function BuiltinMcpRow({ view, onChange }: RowProps) {
    const [running, setRunning] = useState<Running | null>(null);
    // Install and Update are asked first: the command is shown and has to be agreed to.
    const [asking, setAsking] = useState(false);
    const [problem, setProblem] = useState<string | null>(null);
    const [trashing, setTrashing] = useState(false);
    // What an install or a removal prints comes as a hook's does, under no repository.
    const printedFor = hookKey("", `builtin:${view.name}`);
    const forgetHookLine = useStore((state) => state.forgetHookLine);

    const run = (step: BuiltinStep, call: () => Promise<BuiltinMcpView>, key: string | null) => {
        setRunning({ step, key });
        setProblem(null);
        forgetHookLine(printedFor);
        return call()
            .then(onChange)
            .then(
                () => true,
                (error: unknown) => {
                    setProblem(failedLine(step, problemOf(error)));
                    return false;
                },
            )
            .finally(() => setRunning(null));
    };
    const act = (action: BuiltinAction) => {
        if (action === "install" || action === "update") {
            setAsking(true);
        } else {
            void run(action, callFor(action, view, onChange), null);
        }
    };
    const install = () => {
        setAsking(false);
        void run(view.state === "outdated" ? "update" : "install", callFor("install", view), null);
    };
    const trash = () =>
        void run("trash", () => api.builtinMcpTrashOld(view.name), null).then(
            (done) => done && setTrashing(false),
        );

    return (
        <div className="agent-row builtin-row">
            <div className="settings-row">
                <div className="mcp-own">
                    <p className="settings-label">
                        {view.name} <StateMark view={view} />
                    </p>
                    <p className="settings-hint">{summaryLine(view)}</p>
                    <Unpicked view={view} />
                </div>
                {!asking && <Buttons view={view} busy={running !== null} onAct={act} />}
            </div>
            {asking && (
                <InstallAsk view={view} onCancel={() => setAsking(false)} onInstall={install} />
            )}
            <RunningLine view={view} running={running} printedFor={printedFor} />
            {!trashing && (problem ?? view.problem) !== null && (
                <p className="builtin-problem" role="alert">
                    {problem ?? view.problem}
                </p>
            )}
            <StatePart
                view={view}
                running={running}
                printedFor={printedFor}
                onRemove={(key) =>
                    void run("remove", () => api.builtinMcpRemoveRegistration(view.name, key), key)
                }
                onRestart={() =>
                    void run("restart", () => api.builtinMcpRestartSessions(view.name), null)
                }
                onTrash={() => {
                    setProblem(null);
                    setTrashing(true);
                }}
            />
            {hookNoted(view) && <HookNote name={view.name} />}
            {trashing && view.existing && (
                <TrashDialog
                    view={view}
                    existing={view.existing}
                    busy={running?.step === "trash"}
                    problem={problem}
                    onTrash={trash}
                    onKeep={() => setTrashing(false)}
                />
            )}
        </div>
    );
}

/** What a button runs. Install and Update are asked about first, so they come here once agreed. */
function callFor(
    action: BuiltinAction,
    view: BuiltinMcpView,
    onChange?: (view: BuiltinMcpView) => void,
): () => Promise<BuiltinMcpView> {
    const { name } = view;
    switch (action) {
        case "install":
        case "update":
            return () => api.builtinMcpInstall(name);
        case "copy":
            return () => api.builtinMcpTurnOn(name, true);
        case "fresh":
        case "turn-on":
            return () => turnOn(view, onChange);
        case "check-again":
            return () => api.builtinMcpCheck(name, false);
        case "check":
            return () => api.builtinMcpCheck(name, true);
        case "open":
            return () => api.builtinMcpOpen(name).then(() => ({ ...view, page_open: true }));
        case "turn-off":
            return () => api.builtinMcpTurnOff(name);
    }
}

/**
 * Turns on without a copy. A fresh start then opens the server's own page, where the user picks
 * what it reads; the row shows the built-in on even if the page fails to open.
 */
async function turnOn(
    view: BuiltinMcpView,
    onChange?: (view: BuiltinMcpView) => void,
): Promise<BuiltinMcpView> {
    const on = await api.builtinMcpTurnOn(view.name, false);
    if (!startsFresh(view, false)) {
        return on;
    }
    onChange?.(on);
    await api.builtinMcpOpen(view.name);
    return { ...on, page_open: true };
}

/** What a fresh start still needs: the repositories, picked in the server's page. */
function Unpicked({ view }: { view: BuiltinMcpView }) {
    const line = unpickedLine(view);
    return line === null ? null : <p className="settings-hint">{line}</p>;
}

/** The state beside the name, coloured by whether it wants something, and the version. */
function StateMark({ view }: { view: BuiltinMcpView }) {
    const tone = stateTone(view.state);
    return (
        <>
            <span className={tone === "plain" ? "mcp-status" : `mcp-status mcp-status-${tone}`}>
                {stateLabel(view.state)}
            </span>
            {view.version !== null && <span className="builtin-version">{view.version}</span>}
        </>
    );
}

function Buttons(props: {
    view: BuiltinMcpView;
    busy: boolean;
    onAct(action: BuiltinAction): void;
}) {
    const { view, busy, onAct } = props;
    return (
        <div className="builtin-actions">
            {offered(view).map((action) => (
                <button
                    key={action}
                    type="button"
                    className={isMain(action) ? "button button-primary" : "button"}
                    disabled={busy}
                    onClick={() => onAct(action)}
                >
                    {actionLabel(action, view.name).idle}
                </button>
            ))}
        </div>
    );
}

/** Install or Update, asked first with the command it runs, as an agent's install is. */
function InstallAsk(props: { view: BuiltinMcpView; onCancel(): void; onInstall(): void }) {
    const { view, onCancel, onInstall } = props;
    return (
        <div className="agent-ask">
            <p>
                This runs <code>{view.install_command}</code> on this Mac.
            </p>
            <button type="button" className="button" onClick={onCancel}>
                Not now
            </button>
            <button type="button" className="button button-primary" onClick={onInstall}>
                {view.state === "outdated" ? "Update" : "Install"}
            </button>
        </div>
    );
}

/**
 * What runs, drawn moving: an install's last printed line, or a line saying what runs. A
 * removal prints under its own registration, and the Trash in its dialog.
 */
function RunningLine(props: { view: BuiltinMcpView; running: Running | null; printedFor: string }) {
    const { view, running, printedFor } = props;
    if (running === null || running.step === "remove" || running.step === "trash") {
        return null;
    }
    if (printsOutput(running.step)) {
        return <Printed printedFor={printedFor} />;
    }
    return <BusyLine>{busyLine(running.step, view.name)}</BusyLine>;
}

/** The last line a step printed, as an agent's install shows it. */
function Printed({ printedFor }: { printedFor: string }) {
    const printed = useStore((state) => state.hookLines[printedFor] ?? null);
    return (
        <p className="agent-output mono builtin-busy" role="status">
            {printed ?? "Starting…"}
        </p>
    );
}

/** A line saying what runs, with a dot that pulses before it. */
function BusyLine({ children }: { children: ReactNode }) {
    return (
        <p className="builtin-busy" role="status">
            {children}
        </p>
    );
}

interface StatePartProps {
    view: BuiltinMcpView;
    running: Running | null;
    printedFor: string;
    onRemove(key: string): void;
    onRestart(): void;
    onTrash(): void;
}

/** What the state holds under the row's head: the setup to copy, what is left, or the figures. */
function StatePart(props: StatePartProps) {
    const { view } = props;
    if (view.state === "off" && offersCopy(view) && view.existing) {
        return <ExistingPart existing={view.existing} />;
    }
    if (view.state === "waiting") {
        return <WaitingPart {...props} />;
    }
    if (view.state === "on") {
        return <OnPart view={view} busy={props.running !== null} onTrash={props.onTrash} />;
    }
    return null;
}

function ExistingPart({ existing }: { existing: ExistingSetup }) {
    return (
        <div className="builtin-part">
            <p className="settings-hint">{existingLine(existing)}</p>
            <Folders folders={existing.folders} />
        </div>
    );
}

function Folders({ folders }: { folders: string[] }) {
    return (
        <ul className="builtin-folders">
            {folders.map((folder) => (
                <li key={folder} className="mono builtin-folder">
                    {folder}
                </li>
            ))}
        </ul>
    );
}

/** What holds the built-in back: each registration, the old server, and the sessions on it. */
function WaitingPart(props: StatePartProps) {
    const { view, running, printedFor, onRemove, onRestart } = props;
    return (
        <ul className="builtin-steps" aria-label="What is left">
            {view.registrations.map((registration) => (
                <RegistrationItem
                    key={registration.key}
                    registration={registration}
                    busy={running !== null}
                    removing={running?.step === "remove" && running.key === registration.key}
                    printedFor={printedFor}
                    onRemove={() => onRemove(registration.key)}
                />
            ))}
            {view.old_server !== null && (
                <li className="builtin-step">
                    <p className="settings-hint">{oldServerLine(view.old_server)}</p>
                </li>
            )}
            {view.sessions.length > 0 && (
                <HeldItem sessions={view.sessions} busy={running !== null} onRestart={onRestart} />
            )}
        </ul>
    );
}

interface RegistrationProps {
    registration: BuiltinRegistration;
    busy: boolean;
    removing: boolean;
    printedFor: string;
    onRemove(): void;
}

/** A registration outside Swarmed: where it is, and the command that takes it out, or the step. */
function RegistrationItem(props: RegistrationProps) {
    const { registration, busy, removing, printedFor, onRemove } = props;
    const step = registrationStep(registration);
    return (
        <li className="builtin-step">
            <div className="builtin-step-text">
                <p className="settings-label">{registrationPlace(registration)}</p>
                {step.kind === "run" ? (
                    <p className="settings-hint">
                        Remove runs <code>{step.command}</code>
                    </p>
                ) : (
                    <p className="settings-hint">
                        {step.text} {step.command !== null && <code>{step.command}</code>}
                    </p>
                )}
                {removing && <Printed printedFor={printedFor} />}
            </div>
            {step.kind === "run" && (
                <button type="button" className="button" disabled={busy} onClick={onRemove}>
                    Remove
                </button>
            )}
        </li>
    );
}

function HeldItem(props: { sessions: HeldSession[]; busy: boolean; onRestart(): void }) {
    const { sessions, busy, onRestart } = props;
    const words = useWords();
    return (
        <li className="builtin-step">
            <p className="settings-hint builtin-step-text">
                {heldLine(sessions, words.session, words.untitled)}
            </p>
            {restartOffered(sessions) && (
                <button type="button" className="button" disabled={busy} onClick={onRestart}>
                    Restart idle ones
                </button>
            )}
        </li>
    );
}

/** A built-in that is on: what Claude Code said of it, its folder, and the checks. */
function OnPart(props: { view: BuiltinMcpView; busy: boolean; onTrash(): void }) {
    const { view, busy, onTrash } = props;
    const line = checksLine(view);
    return (
        <div className="builtin-part">
            <dl className="usage">
                <Fact name="Claude Code says">
                    <ClaudeSays status={view.status} />
                </Fact>
                <Fact name="Folder">
                    <span className="mono">{view.home}</span>
                </Fact>
            </dl>
            {view.page_open && (
                <p className="settings-hint">
                    Its page runs until Swarmed quits. Open {view.name} shows it again.
                </p>
            )}
            {line !== null && <p className="settings-hint">{line}</p>}
            <ul className="builtin-checks">
                {view.checks.map((check) => (
                    <CheckItem key={check.name} check={check} />
                ))}
            </ul>
            {trashOffered(view) && view.existing && (
                <div className="settings-row">
                    <p className="settings-hint">
                        Your old setup is no longer used. Its folders can go to the Trash.
                    </p>
                    <button type="button" className="button" disabled={busy} onClick={onTrash}>
                        Move the old setup to the Trash
                    </button>
                </div>
            )}
        </div>
    );
}

function ClaudeSays({ status }: { status: string | null }) {
    const { said, tone } = statusMark(status);
    return (
        <span className={tone === "plain" ? "mcp-status" : `mcp-status mcp-status-${tone}`}>
            {said}
        </span>
    );
}

function Fact({ name, children }: { name: string; children: ReactNode }) {
    return (
        <div className="usage-row">
            <dt>
                <span className="usage-name">{name}</span>
            </dt>
            <dd>{children}</dd>
        </div>
    );
}

/** What each check's mark says, by how it came out. */
const CHECK_MARKS = {
    good: { said: "passes", className: "mcp-status mcp-status-good" },
    warning: { said: "warning", className: "mcp-status mcp-status-off" },
    failing: { said: "fails", className: "mcp-status builtin-failing" },
} as const;

/** One of the doctor's checks: its name, how it came out, what it found, and its fix. */
function CheckItem({ check }: { check: DoctorCheck }) {
    const mark = CHECK_MARKS[checkTone(check)];
    const fix = shownFix(check);
    return (
        <li className="builtin-check">
            <span className="builtin-check-name">{check.name}</span>
            <span className={mark.className}>{mark.said}</span>
            <span className="builtin-check-detail">{check.detail}</span>
            {fix !== null && (
                <span className="builtin-check-fix">
                    Fix: <code>{fix}</code>
                </span>
            )}
        </li>
    );
}

/** The refresh hook in Claude Code's settings: pointed out, since Swarmed edits no agent's files. */
function HookNote({ name }: { name: string }) {
    return (
        <p className="settings-hint builtin-note">
            Claude Code's settings still have a <code>{name} session-start</code> hook, which
            refreshes the old index. Swarmed refreshes this one itself as each session starts, so
            the hook can go: take it out of Claude Code's settings yourself, since Swarmed does not
            edit them.
        </p>
    );
}

interface TrashProps {
    view: BuiltinMcpView;
    existing: ExistingSetup;
    busy: boolean;
    problem: string | null;
    onTrash(): void;
    onKeep(): void;
}

/** Asked before the old setup goes: the exact folders, and where the built-in goes on from. */
function TrashDialog({ view, existing, busy, problem, onTrash, onKeep }: TrashProps) {
    return (
        <Dialog
            title="Move the old setup to the Trash?"
            size="small"
            onClose={onKeep}
            dismiss="Keep it"
            busy={busy}
            problem={problem}
            actions={
                <button type="button" className="button button-danger" onClick={onTrash}>
                    Move to the Trash
                </button>
            }
        >
            <p>
                These folders go to the Trash, where they can be put back from until it is emptied:
            </p>
            <Folders folders={existing.folders} />
            <p>
                {view.name} goes on from its own copy, in <code>{view.home}</code>.
            </p>
            {busy && <BusyLine>{busyLine("trash", view.name)}</BusyLine>}
        </Dialog>
    );
}

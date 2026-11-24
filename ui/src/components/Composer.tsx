import { lazy, Suspense, useEffect, useRef } from "react";
import { sendOrQueue } from "../actions";
import { api } from "../api";
import type { AgentEvent, Session } from "../api/types";
import { noteCommandSent } from "../commandSent";
import { expandPastes, forgetPastes } from "../pastes";
import { browse, type Direction, sentMessages } from "../recall";
import { type Draft, filesWith, NO_DRAFT, waitsItsTurn } from "../sending";
import { takenBySwarmed } from "../slash";
import { appliedEffort } from "../statusline";
import { useStore } from "../store";
import { suggestionOf } from "../suggestion";
import { useWords } from "../useWords";
import { capitalized } from "../wording";
import { ComposerBox } from "./ComposerBox";
import { EffortPicker } from "./EffortPicker";
import { FastToggle } from "./FastToggle";
import { MessageQueue } from "./MessageQueue";
import { ModePicker, TerminalIcon } from "./ModePicker";
import {
    CLAUDE,
    modelChoices,
    takesEffort,
    takesFast,
    useAgents,
    useEffortDefault,
    useHearsMidTurn,
    useSlashCommands,
} from "./options";
import { Picker } from "./Picker";

// The terminal emulator is a large library; it is only fetched when a terminal is first shown.
const TerminalBox = lazy(async () => {
    const module = await import("./TerminalBox");
    return { default: module.TerminalBox };
});

const PLACEHOLDER: Record<Session["status"], string> = {
    // The style's own words, in place of this.
    idle: "Reply to the agent",
    working:
        "Write the next message: it waits until the agent is done. Esc stops the agent, ⌘⇧Enter stops it and sends this now.",
    waiting: "Answer the request above. A message written here waits until the agent is done.",
    errored: "Send a message to start the agent again",
};

/**
 * The session's terminal in place of the message box: its shell, whose output goes to the agent,
 * or Claude Code opened on a command's screen, whose output does not.
 */
function TerminalPane({ id, busy }: { id: number; busy: boolean }) {
    const claude = useStore((state) => state.claudeTerminals[id]);
    const toggleTerminal = useStore((state) => state.toggleTerminal);
    const leaveClaudeTerminal = useStore((state) => state.leaveClaudeTerminal);
    const report = useStore((state) => state.report);
    const words = useWords();
    const share = () => api.sessionSend(id, "I ran this in the terminal.").catch(report);
    const backToShell = () => {
        leaveClaudeTerminal(id);
        api.claudeTerminalClose(id).catch(report);
    };
    return (
        <div className="terminal-pane">
            <Suspense fallback={<div className="terminal-box" />}>
                <TerminalBox key={claude ?? ""} sessionId={id} claude={claude} />
            </Suspense>
            <div className="terminal-foot">
                {claude === undefined ? (
                    <>
                        <p>What you run here goes to the agent with your next message.</p>
                        <button type="button" className="button" disabled={busy} onClick={share}>
                            Send to the {words.terms.agent} now
                        </button>
                    </>
                ) : (
                    <>
                        <p>
                            Claude Code, opened on <span className="mono">/{claude}</span>: what it
                            shows stays here, and is not sent to the agent.
                        </p>
                        <button type="button" className="button" onClick={backToShell}>
                            {words.backToShell}
                        </button>
                    </>
                )}
                <button type="button" className="button" title="Ctrl+`" onClick={toggleTerminal}>
                    Back to the message
                </button>
            </div>
        </div>
    );
}

/**
 * The message box of an existing session. What it holds is the session's draft in the store,
 * so it is still there after another session was opened, and no other session sees it.
 */
export function Composer({ session }: { session: Session }) {
    const words = useWords();
    const { id, status } = session;
    const draft = useStore((state) => state.drafts[id] ?? NO_DRAFT);
    const { text, files, images } = draft;
    const editDraft = useStore((state) => state.editDraft);
    const putDraftBack = useStore((state) => state.putDraftBack);
    const terminal = useStore((state) => state.terminalOpen);
    const toggleTerminal = useStore((state) => state.toggleTerminal);
    const report = useStore((state) => state.report);
    const input = useRef<HTMLTextAreaElement>(null);
    const recalling = useRecall(id, session.spawned_by !== null, draft);
    const busy = waitsItsTurn(status);
    const commands = useSlashCommands(id, status);
    const suggestion = useStore((state) => suggestionOf(state.events[id]));
    const canSend = text.trim() !== "" || files.length > 0 || images.length > 0;
    const hearsMidTurn = useHearsMidTurn(session.agent);

    // Answering a permission request removes the button that had focus. Bring it back here, so
    // the keyboard is never left pointing at nothing.
    // biome-ignore lint/correctness/useExhaustiveDependencies: runs when the status changes
    useEffect(() => {
        if (document.activeElement === document.body) {
            input.current?.focus();
        }
    }, [status]);

    const send = (now = false) => {
        if (!canSend) {
            return;
        }
        let unsent: Draft = draft;
        editDraft(id, NO_DRAFT);
        recalling.forget();
        // What was pasted large goes out in full, in place of the token that stood for it.
        // A busy agent is not interrupted: the message waits its turn, and can still be changed.
        const message = expandPastes(text.trim());
        const atOnce =
            files.length === 0 && images.length === 0 && takenBySwarmed(message, commands);
        if (atOnce) {
            noteCommandSent(id);
        }
        // Pasted images are kept as the session's files first, and go as their paths.
        filesWith(api, id, draft)
            .then((attached) => {
                // Kept now: a message that comes back has them as files, not to be kept twice.
                unsent = { ...draft, files: attached, images: [] };
                return sendOrQueue(id, message, attached, atOnce, now);
            })
            // The pastes are only forgotten once they are sent: a message that comes back
            // still has their tokens in it.
            .then(() => forgetPastes(unsent.text))
            .catch((error: unknown) => {
                report(error);
                // Into this session's box, even if another session is on screen by now.
                putDraftBack(id, unsent);
            });
    };
    const stop = () => void api.sessionInterrupt(id).catch(report);

    return (
        <div className="composer">
            {terminal ? (
                <TerminalPane id={id} busy={busy} />
            ) : (
                <>
                    <MessageQueue sessionId={id} busy={busy} hearsMidTurn={hearsMidTurn} />
                    <ComposerBox
                        value={text}
                        onChange={(value) => editDraft(id, { text: value })}
                        files={files}
                        onFiles={(value) => editDraft(id, { files: value })}
                        images={images}
                        onImages={(value) => editDraft(id, { images: value })}
                        sessionId={id}
                        onSend={() => send()}
                        onSendNow={() => send(true)}
                        onStop={status === "working" ? stop : undefined}
                        canSend={canSend}
                        placeholder={status === "idle" ? words.reply : PLACEHOLDER[status]}
                        suggestion={busy ? null : suggestion}
                        inputRef={input}
                        commands={commands}
                        onRecall={recalling.recall}
                    >
                        <SessionPickers session={session} />
                        <button
                            type="button"
                            className="icon-picker"
                            aria-label="Terminal"
                            title={`Terminal: a shell in the ${words.terms.task}'s ${words.terms.worktree} (Ctrl+\`)`}
                            onClick={toggleTerminal}
                        >
                            <TerminalIcon />
                        </button>
                    </ComposerBox>
                </>
            )}
        </div>
    );
}

/**
 * What ↑ and ↓ do in session `id`'s message box, which holds `now`: go back through what was
 * sent to the session, and forward again to what was typed. Where that stands is kept in the
 * store beside the draft, so what was typed before ↑ is still there to come back to after
 * another session was opened. `spawned` says another session started this one.
 */
function useRecall(id: number, spawned: boolean, now: Draft) {
    const editDraft = useStore((state) => state.editDraft);
    const noteRecall = useStore((state) => state.noteRecall);
    return {
        recall: (direction: Direction): Draft | null => {
            const { events, recalls } = useStore.getState();
            // The events are read already, for the transcript; the list is made from them only
            // when ↑ starts going back.
            const sent = () => sentMessages(events[id] ?? [], spawned);
            const next = browse(recalls[id] ?? null, direction, now, sent);
            if (next !== null) {
                noteRecall(id, next);
                editDraft(id, next.shown);
            }
            return next?.shown ?? null;
        },
        /** A message was sent: ↑ starts again from the newest. */
        forget: () => noteRecall(id, null),
    };
}

/**
 * Whether a session has talked to its agent, or is about to: the agent then holds the
 * conversation, and the session stays on it. A fork has not, though it shows a conversation.
 */
function hasBegun(session: Session, events: AgentEvent[] | undefined): boolean {
    const started = session.agent_session_id !== null || session.status !== "idle";
    const forked = events?.findLastIndex((event) => event.type === "forked") ?? -1;
    const said = events?.slice(forked + 1).some((event) => event.type === "user_message");
    return started || Boolean(said);
}

/** The session's supervision, model and effort, changeable at any time, also while the agent works. */
function SessionPickers({ session }: { session: Session }) {
    const words = useWords();
    const configure = useStore((state) => state.configure);
    const setAgent = useStore((state) => state.setAgent);
    const agents = useAgents();
    const agent = agents.find((candidate) => candidate.id === session.agent);
    // Until the first message reaches an agent, which agent that is can still be chosen.
    const begun = useStore((state) => hasBegun(session, state.events[session.id]));
    const choosable = !begun && agents.length > 1;
    const { id, model, supervision, effort, fast } = session;
    // What the agent last said its effort comes to, for when none was chosen.
    const said = useStore((state) => appliedEffort(state.events[id]));
    // Before it has said, what it said the last time a session left the effort to it.
    const usual = useEffortDefault(session.agent, model);
    const applied = said ?? usual;
    const current = { model, supervision, effort, fast };
    return (
        <>
            <ModePicker
                value={supervision}
                agentId={session.agent}
                onChange={(value) => void configure(api, id, { ...current, supervision: value })}
            />
            {choosable && (
                <Picker
                    label={capitalized(words.terms.agent)}
                    value={session.agent}
                    onChange={(value) => void setAgent(api, id, value)}
                    options={agents.map((candidate) => [candidate.id, candidate.name])}
                />
            )}
            {!choosable && session.agent !== CLAUDE && (
                <span className="option-label">{agent?.name ?? session.agent}</span>
            )}
            <Picker
                label={capitalized(words.terms.model)}
                value={model ?? ""}
                onChange={(value) => void configure(api, id, { ...current, model: value || null })}
                options={modelChoices(agent, model, words)}
            />
            {/* Changing it restarts the agent between turns. */}
            {takesEffort(session.agent) && (
                <EffortPicker
                    value={effort}
                    applied={applied}
                    onChange={(value) => void configure(api, id, { ...current, effort: value })}
                />
            )}
            {takesFast(session.agent) && (
                <FastToggle
                    on={fast}
                    onChange={(value) => void configure(api, id, { ...current, fast: value })}
                />
            )}
        </>
    );
}

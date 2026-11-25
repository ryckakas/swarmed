import { useEffect, useMemo, useState } from "react";
import { rewindSession } from "../actions";
import { api } from "../api";
import type { FileChange, RewindCheck } from "../api/types";
import { commitsSaid, rewindChoices } from "../rewind";
import { type Rewinding, useStore } from "../store";
import { useWords } from "../useWords";
import { capitalized } from "../wording";
import { Dialog } from "./Dialog";

/** How many of the files that would go back are named; the rest are counted. */
const FILES_NAMED = 8;

/** What becomes of a file as the files go back, by how it differs from then to now. */
const GOING: Record<FileChange["kind"], string> = {
    added: "comes back",
    deleted: "is deleted",
    modified: "changes back",
};

/** A reason as the start of a line, the core's words otherwise as they are. */
function capitalizedOrNull(reason: string | null): string | null {
    return reason === null ? null : capitalized(reason);
}

/** What went wrong, in the core's words. */
function said(error: unknown): string {
    return String(error).replace(/^Error: /, "");
}

/**
 * Asks how to take a session back to before one of its messages: the conversation alone, or
 * its files too. Opened from a message's Edit from here, which names the message, or by
 * `/rewind`, which leaves it to be chosen here. Says what cannot be undone, and what the core
 * refuses before anything is tried.
 */
export function RewindDialog() {
    const rewinding = useStore((state) => state.rewinding);
    if (!rewinding) {
        return null;
    }
    const key = `${rewinding.sessionId}-${rewinding.message ?? "choose"}`;
    return <RewindAsked key={key} rewinding={rewinding} />;
}

function RewindAsked({ rewinding }: { rewinding: Rewinding }) {
    const { sessionId } = rewinding;
    const askToRewind = useStore((state) => state.askToRewind);
    const events = useStore((state) => state.events[sessionId]);
    const choices = useMemo(() => rewindChoices(events ?? []), [events]);
    const [message, setMessage] = useState(rewinding.message ?? choices[0]?.number ?? null);
    const [files, setFiles] = useState(false);
    const [busy, setBusy] = useState(false);
    const [problem, setProblem] = useState<string | null>(null);
    const check = useCheck(sessionId, message, setProblem);
    // Files a message cannot take back are not taken back, whatever was ticked before it.
    const withFiles = files && check?.files_refused === null;
    const close = () => askToRewind(null);
    const go = () => {
        if (message === null) {
            return;
        }
        setBusy(true);
        setProblem(null);
        rewindSession(sessionId, message, withFiles)
            .then(close)
            .catch((error: unknown) => {
                setProblem(said(error));
                setBusy(false);
            });
    };
    const refused = check?.refused ?? null;
    return (
        <Dialog
            title={
                rewinding.message === null
                    ? "Take the session back to before which message?"
                    : "Take the session back to before this message?"
            }
            size="medium"
            onClose={close}
            dismiss="Keep it as it is"
            busy={busy}
            problem={capitalizedOrNull(problem ?? refused)}
            actions={
                <button
                    type="button"
                    className="button button-primary"
                    disabled={busy || check === null || refused !== null}
                    onClick={go}
                >
                    {withFiles ? "Take back, files too" : "Take back"}
                </button>
            }
        >
            {rewinding.message === null && (
                <MessagePicker choices={choices} message={message} onMessage={setMessage} />
            )}
            {rewinding.message !== null && (
                <p className="dialog-subject">
                    {choices.find((one) => one.number === message)?.text ?? ""}
                </p>
            )}
            {message === null && <p>Nothing has been sent to this session yet.</p>}
            {check !== null && refused === null && (
                <RewindChoices check={check} files={withFiles} onFiles={setFiles} />
            )}
        </Dialog>
    );
}

/** What taking the session back to before `message` would do, read again as it changes. */
function useCheck(
    sessionId: number,
    message: number | null,
    onProblem: (problem: string | null) => void,
): RewindCheck | null {
    const [check, setCheck] = useState<RewindCheck | null>(null);
    useEffect(() => {
        setCheck(null);
        if (message === null) {
            return;
        }
        let current = true;
        api.sessionRewindCheck(sessionId, message)
            .then((read) => current && setCheck(read))
            .catch((error: unknown) => current && onProblem(said(error)));
        return () => {
            current = false;
        };
    }, [sessionId, message, onProblem]);
    return check;
}

/** The messages `/rewind` can take the session back to before, the latest first. */
function MessagePicker(props: {
    choices: { number: number; text: string }[];
    message: number | null;
    onMessage(message: number): void;
}) {
    const { choices, message, onMessage } = props;
    return (
        <fieldset className="rewind-messages">
            <legend>Back to before</legend>
            {choices.map((choice) => (
                <label key={choice.number} className="rewind-choice">
                    <input
                        type="radio"
                        name="rewind-message"
                        checked={choice.number === message}
                        onChange={() => onMessage(choice.number)}
                    />
                    {choice.text || "(a message with files only)"}
                </label>
            ))}
        </fieldset>
    );
}

/** The two ways to go back, with what each does and what cannot be undone. */
function RewindChoices(props: {
    check: RewindCheck;
    files: boolean;
    onFiles(files: boolean): void;
}) {
    const { check, files, onFiles } = props;
    const words = useWords();
    const { agent, commit, push, pullRequest, worktree } = words.terms;
    const filesOut = check.files_refused;
    const commits = commitsSaid(check.commits_since);
    return (
        <>
            <p>
                The message and what followed it no longer count: they stay in the transcript,
                folded, and the message goes back into the box to edit and send again.{" "}
                {check.resumes
                    ? `The ${agent} resumes its own conversation just before it.`
                    : `The ${agent} starts a new conversation and is told what was said up to it.`}
            </p>
            <fieldset className="rewind-ways">
                <label className="rewind-choice">
                    <input
                        type="radio"
                        name="rewind-files"
                        checked={!files}
                        onChange={() => onFiles(false)}
                    />
                    <span>
                        <strong>Conversation only</strong>: the files stay as they are now.
                    </span>
                </label>
                <label className="rewind-choice">
                    <input
                        type="radio"
                        name="rewind-files"
                        checked={files}
                        disabled={filesOut !== null}
                        onChange={() => onFiles(true)}
                    />
                    <span>
                        <strong>Conversation and files</strong>: the {worktree} goes back to how it
                        was before this message, and can be put back afterwards.
                    </span>
                </label>
            </fieldset>
            {filesOut !== null && <p className="aside">The files cannot go back: {filesOut}.</p>}
            {filesOut === null && <FilesGoingBack check={check} />}
            <p className="aside">
                {commits ?? `Any ${commit} stays on the branch: only the files go back.`} Nothing
                outside the {worktree} comes back: a {push}, a {pullRequest}, a database, a package
                installed elsewhere. Files git ignores are left as they are.
            </p>
        </>
    );
}

/** The files that would go back, named up to a few. */
function FilesGoingBack({ check }: { check: RewindCheck }) {
    if (check.files.length === 0) {
        return <p className="aside">No file differs from how it was before this message.</p>;
    }
    const named = check.files.slice(0, FILES_NAMED);
    const more = check.files.length - named.length;
    return (
        <div className="rewind-files">
            <p className="aside">
                With the files: {check.files.length}{" "}
                {check.files.length === 1 ? "file goes" : "files go"} back.
            </p>
            <ul>
                {named.map((file) => (
                    <li key={file.path}>
                        <span className="mono">{file.path}</span>: {GOING[file.kind]}
                    </li>
                ))}
                {more > 0 && <li>and {more} more</li>}
            </ul>
        </div>
    );
}

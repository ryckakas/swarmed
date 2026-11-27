import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import {
    APPLY_AFTER_MS,
    type ChangedLine,
    changedLines,
    changeSaid,
    shownLine,
    stylesheetNotice,
} from "../stylesheet";

/**
 * What the window says of the user's own stylesheet, at its top: that its file changed on disk,
 * with what changed, and is applied only once the user says so; or that this run leaves it out
 * because ⌥ was held as Swarmed started. An agent can write the file, and CSS can hide or fake
 * anything in the window, so nothing of a change on disk is applied unseen (`stylesheet.rs`).
 */
export function StylesheetNotice() {
    const sheet = useStore((state) => state.stylesheet);
    const aside = useStore((state) => state.stylesheetAside);
    const putAside = useStore((state) => state.putStylesheetAside);
    const leaveOut = useStore((state) => state.leaveStylesheetOut);
    const use = useStore((state) => state.useStylesheet);
    const notice = stylesheetNotice(sheet, aside);
    const again = useChangedAgain(notice?.kind === "changed" ? notice.file : null);
    if (notice === null) {
        return null;
    }
    if (notice.kind === "changed") {
        return (
            <ChangedOnDisk
                // A new text is a new notice: nothing of the one before carries over to it.
                key={notice.file}
                file={notice.file}
                accepted={notice.accepted}
                again={again}
                onLater={() => putAside({ file: notice.file })}
            />
        );
    }
    return (
        <div className="stylesheet-notice" role="status">
            <div className="stylesheet-notice-head">
                <span className="stylesheet-notice-text">
                    Your stylesheet is left out until Swarmed quits: ⌥ was held as it started.
                </span>
                <button type="button" className="button" onClick={() => void use(api, false)}>
                    Turn it off
                </button>
                <button type="button" className="button" onClick={() => void leaveOut(api, false)}>
                    Use it again
                </button>
                <button
                    type="button"
                    className="button"
                    onClick={() => putAside({ leftOut: true })}
                >
                    Dismiss
                </button>
            </div>
        </div>
    );
}

/** Whether the change shown, `file`, took the place of another while that one was shown. */
export function useChangedAgain(file: string | null): boolean {
    const before = useRef<string | null>(null);
    const again = file !== null && before.current !== null && before.current !== file;
    useEffect(() => {
        before.current = file;
    });
    return again;
}

interface ChangedOnDiskProps {
    /** The file as it is shown, which is what Apply accepts. */
    file: string;
    /** What the user accepted before, which the change is shown against. */
    accepted: string;
    /** Whether it took the place of another change while that one was shown. */
    again: boolean;
    /** Puts it aside until the file changes again; without it there is no Not now. */
    onLater?: () => void;
}

/**
 * A change of the stylesheet's file, shown line by line, applied once the user says so. Mounted
 * anew for each text, so Apply always takes what is on screen; and it waits a moment before it
 * can be pressed, so that a click meant for the change before cannot take this one.
 */
export function ChangedOnDisk({ file, accepted, again, onLater }: ChangedOnDiskProps) {
    const accept = useStore((state) => state.acceptStylesheet);
    const [wasAgain] = useState(again);
    const [folded, setFolded] = useState(false);
    const [ready, setReady] = useState(false);
    const [problem, setProblem] = useState<string | null>(null);
    useEffect(() => {
        const timer = setTimeout(() => setReady(true), APPLY_AFTER_MS);
        return () => clearTimeout(timer);
    }, []);
    const lines = changedLines(accepted, file);
    const apply = async () => setProblem(await accept(api, file));
    return (
        <div className="stylesheet-notice" role="alert">
            <div className="stylesheet-notice-head">
                <span className="stylesheet-notice-text">
                    Your stylesheet changed on disk{wasAgain ? " again" : ""} ({changeSaid(lines)}
                    ). It is applied once you say so.
                </span>
                <button
                    type="button"
                    className="button"
                    aria-expanded={!folded}
                    onClick={() => setFolded(!folded)}
                >
                    {folded ? "Show the change" : "Hide the change"}
                </button>
                <button
                    type="button"
                    className="button button-primary"
                    disabled={!ready}
                    onClick={apply}
                >
                    Apply
                </button>
                {onLater && (
                    <button type="button" className="button" onClick={onLater}>
                        Not now
                    </button>
                )}
            </div>
            {problem && <p className="stylesheet-notice-problem">{problem}</p>}
            {!folded && <Change lines={lines} />}
        </div>
    );
}

/** The lines a change took out and put in, with a gap where unchanged ones were left out. */
function Change({ lines }: { lines: ChangedLine[] }) {
    return (
        <div className="stylesheet-change diff">
            <div className="diff-lines">
                {lines.map((line, index) => (
                    // biome-ignore lint/suspicious/noArrayIndexKey: the lines of one change never reorder
                    <div key={index} className={`diff-line ${LINE_CLASS[line.kind]}`}>
                        <span className="diff-sign">{SIGN[line.kind]}</span>
                        {line.kind === "gap" ? "⋯" : shownLine(line.text)}
                    </div>
                ))}
            </div>
        </div>
    );
}

const LINE_CLASS: Record<ChangedLine["kind"], string> = {
    added: "diff-added",
    removed: "diff-removed",
    gap: "diff-hunk",
};

const SIGN: Record<ChangedLine["kind"], string> = { added: "+", removed: "−", gap: "" };

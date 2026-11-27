import { useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import { type ChangedLine, changedLines, changeSaid, stylesheetNotice } from "../stylesheet";

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
    if (notice === null) {
        return null;
    }
    if (notice.kind === "changed") {
        return <ChangedOnDisk file={notice.file} accepted={notice.accepted} />;
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

/** A change of the stylesheet's file, shown line by line, applied once the user says so. */
function ChangedOnDisk({ file, accepted }: { file: string; accepted: string }) {
    const accept = useStore((state) => state.acceptStylesheet);
    const putAside = useStore((state) => state.putStylesheetAside);
    const [folded, setFolded] = useState(false);
    const [problem, setProblem] = useState<string | null>(null);
    const lines = changedLines(accepted, file);
    // What is applied is the file as shown here, even if it changes again meanwhile.
    const apply = async () => setProblem(await accept(api, file));
    return (
        <div className="stylesheet-notice" role="alert">
            <div className="stylesheet-notice-head">
                <span className="stylesheet-notice-text">
                    Your stylesheet changed on disk ({changeSaid(lines)}). It is applied once you
                    say so.
                </span>
                <button
                    type="button"
                    className="button"
                    aria-expanded={!folded}
                    onClick={() => setFolded(!folded)}
                >
                    {folded ? "Show the change" : "Hide the change"}
                </button>
                <button type="button" className="button button-primary" onClick={apply}>
                    Apply
                </button>
                <button type="button" className="button" onClick={() => putAside({ file })}>
                    Not now
                </button>
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
                        {line.kind === "gap" ? "⋯" : line.text}
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

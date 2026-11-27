import { type ReactNode, useState } from "react";
import { api } from "../api";
import type { Stylesheet } from "../api/types";
import { useStore } from "../store";
import { changedAt, mayKeep, stylesheetBox } from "../stylesheet";
import { Row } from "./SettingsRow";
import { ChangedOnDisk, useChangedAgain } from "./StylesheetNotice";

/** What the stylesheet is for, and what it can rely on, as the hint beside its switch. */
const STYLESHEET_HINT =
    "style.css in Swarmed's data folder, applied after the app's own styles. A change made to the file elsewhere " +
    "waits for your yes, and agents cannot change it. The variables (--text, --sky, --space-2…) stay from version " +
    "to version; class names may not. Hold ⌥ as Swarmed starts to leave it out.";

/**
 * The user's own stylesheet in Settings: on or off, when its file changed, and the CSS itself,
 * which applies at once when saved here: what the user writes here is theirs. The file opens
 * in their editor too, where a change waits for their yes in the window (`stylesheet.rs`).
 */
export function StylesheetSettings({ on }: { on: boolean }) {
    const sheet = useStore((state) => state.stylesheet);
    const use = useStore((state) => state.useStylesheet);
    const when = sheet === null ? "" : changedAt(sheet);
    return (
        <>
            <Row
                label="Your stylesheet"
                hint={when === "" ? STYLESHEET_HINT : `${STYLESHEET_HINT} Changed ${when}.`}
            >
                <input
                    type="checkbox"
                    aria-label="Use my stylesheet"
                    checked={on}
                    onChange={(event) => void use(api, event.target.checked)}
                />
            </Row>
            {on && sheet !== null && <StylesheetEditor sheet={sheet} />}
        </>
    );
}

/**
 * The stylesheet's CSS, starting from what is applied (`stylesheetBox`). Saved here, an edit of
 * the user's own is applied at once. A change on disk not yet accepted is shown apart, line by
 * line with its own Apply, never put in the box, where Save would accept it unseen.
 */
function StylesheetEditor({ sheet }: { sheet: Stylesheet }) {
    const again = useChangedAgain(sheet.waiting ? sheet.file : null);
    const box = stylesheetBox(sheet);
    const [problem, setProblem] = useState<string | null>(null);
    const open = async () => {
        try {
            await api.stylesheetOpen();
            setProblem(null);
        } catch (error) {
            setProblem(String(error));
        }
    };
    const openButton = (
        <button type="button" className="button" onClick={open}>
            Open in editor
        </button>
    );
    return (
        <div className="layout-editor">
            {sheet.waiting && sheet.file !== null && (
                <ChangedOnDisk
                    key={sheet.file}
                    file={sheet.file}
                    accepted={sheet.accepted}
                    again={again}
                />
            )}
            {box.kind === "too_large" ? (
                <>
                    <p className="settings-hint">
                        style.css is larger than {MAX_KB} kB, so it is not read and nothing of it is
                        applied. Open it in your editor to make it shorter.
                    </p>
                    <div className="layout-foot">
                        <p className="layout-problem" role="alert">
                            {problem}
                        </p>
                        {openButton}
                    </div>
                </>
            ) : (
                <CssBox
                    started={box.text}
                    waiting={sheet.waiting}
                    problem={problem}
                    onProblem={setProblem}
                    openButton={openButton}
                />
            )}
        </div>
    );
}

/** The largest stylesheet read, in kB, as the core has it (`MAX_STYLESHEET_BYTES`). */
const MAX_KB = 200;

interface CssBoxProps {
    /** What the box starts from: what is applied. */
    started: string;
    /** Whether the file holds a change not accepted, which saving here writes over. */
    waiting: boolean;
    /** What went wrong last, said beside the buttons. */
    problem: string | null;
    onProblem(problem: string | null): void;
    /** Opens the file in the user's editor, beside Revert and Save. */
    openButton: ReactNode;
}

/** The CSS as the user edits it, with Revert and Save for an edit of their own. */
function CssBox({ started, waiting, problem, onProblem, openButton }: CssBoxProps) {
    const save = useStore((state) => state.saveStylesheet);
    const [typed, setTyped] = useState<string | null>(null);
    const shown = typed ?? started;
    const keep = async () => {
        const wrong = await save(api, shown);
        onProblem(wrong);
        setTyped(wrong === null ? null : shown);
    };
    return (
        <>
            {waiting && (
                <p className="settings-hint">
                    The box holds what is applied now. Saving an edit here writes it over the change
                    on disk.
                </p>
            )}
            <textarea
                className="layout-json"
                aria-label="Your stylesheet, as CSS"
                spellCheck={false}
                placeholder="/* For example: :root { --sky: #7cc4ff; } */"
                value={shown}
                onChange={(event) => setTyped(event.target.value)}
            />
            <div className="layout-foot">
                <p className="layout-problem" role="alert">
                    {problem}
                </p>
                {openButton}
                <button
                    type="button"
                    className="button"
                    disabled={!mayKeep(started, typed)}
                    onClick={() => {
                        setTyped(null);
                        onProblem(null);
                    }}
                >
                    Revert
                </button>
                <button
                    type="button"
                    className="button button-primary"
                    disabled={!mayKeep(started, typed)}
                    onClick={keep}
                >
                    Save stylesheet
                </button>
            </div>
        </>
    );
}

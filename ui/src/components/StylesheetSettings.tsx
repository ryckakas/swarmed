import { useState } from "react";
import { api } from "../api";
import type { Stylesheet } from "../api/types";
import { useStore } from "../store";
import { changedAt } from "../stylesheet";
import { Row } from "./SettingsRow";

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

/** The stylesheet's CSS as its file holds it. Saved here, it is applied at once. */
function StylesheetEditor({ sheet }: { sheet: Stylesheet }) {
    const save = useStore((state) => state.saveStylesheet);
    const [text, setText] = useState<string | null>(null);
    const [problem, setProblem] = useState<string | null>(null);
    const written = sheet.file ?? "";
    const shown = text ?? written;
    const keep = async () => {
        const wrong = await save(api, shown);
        setProblem(wrong);
        setText(wrong === null ? null : shown);
    };
    const open = async () => {
        try {
            await api.stylesheetOpen();
            setProblem(null);
        } catch (error) {
            setProblem(String(error));
        }
    };
    return (
        <div className="layout-editor">
            {sheet.waiting && (
                <p className="settings-hint">
                    The file changed since you last applied it, and the window asks about it. Saving
                    here applies what is shown.
                </p>
            )}
            <textarea
                className="layout-json"
                aria-label="Your stylesheet, as CSS"
                spellCheck={false}
                placeholder="/* For example: :root { --sky: #7cc4ff; } */"
                value={shown}
                onChange={(event) => setText(event.target.value)}
            />
            <div className="layout-foot">
                <p className="layout-problem" role="alert">
                    {problem}
                </p>
                <button type="button" className="button" onClick={open}>
                    Open in editor
                </button>
                <button
                    type="button"
                    className="button"
                    disabled={shown === written}
                    onClick={() => {
                        setText(null);
                        setProblem(null);
                    }}
                >
                    Revert
                </button>
                <button
                    type="button"
                    className="button button-primary"
                    disabled={shown === written && !sheet.waiting}
                    onClick={keep}
                >
                    Save stylesheet
                </button>
            </div>
        </div>
    );
}

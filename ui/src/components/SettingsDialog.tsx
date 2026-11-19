import { type ReactNode, useEffect, useRef } from "react";
import { api } from "../api";
import type { Settings, Theme } from "../api/types";
import { useStore } from "../store";
import { editorChoices, TEXT_SCALES, THEMES } from "./options";
import { Picker } from "./Picker";

/** The user's preferences. Every change takes effect, and is saved, at once. */
export function SettingsDialog() {
    const open = useStore((state) => state.settingsOpen);
    const settings = useStore((state) => state.settings);
    const showSettings = useStore((state) => state.showSettings);
    const dialog = useRef<HTMLDialogElement>(null);
    const shown = open && settings !== null;

    useEffect(() => {
        if (shown && !dialog.current?.open) {
            dialog.current?.showModal();
        }
    }, [shown]);

    if (!(shown && settings)) {
        return null;
    }
    return (
        <dialog
            ref={dialog}
            className="confirm settings"
            aria-labelledby="settings-title"
            onClose={() => showSettings(false)}
        >
            <h2 id="settings-title">Settings</h2>
            <Appearance settings={settings} />
            <Sessions settings={settings} />
            <div className="confirm-actions">
                <button type="button" className="button" onClick={() => showSettings(false)}>
                    Done
                </button>
            </div>
        </dialog>
    );
}

function Appearance({ settings }: { settings: Settings }) {
    const change = useStore((state) => state.changeSettings);
    return (
        <section className="settings-group" aria-label="Appearance">
            <h3>Appearance</h3>
            <Row label="Colours">
                <Picker
                    label="Colours"
                    value={settings.theme}
                    onChange={(theme) => void change(api, { theme: theme as Theme })}
                    options={THEMES}
                />
            </Row>
            <Row label="Text size">
                <Picker
                    label="Text size"
                    value={String(settings.text_scale)}
                    onChange={(scale) => void change(api, { text_scale: Number(scale) })}
                    options={withCurrent(TEXT_SCALES, String(settings.text_scale))}
                />
            </Row>
        </section>
    );
}

function Sessions({ settings }: { settings: Settings }) {
    const change = useStore((state) => state.changeSettings);
    return (
        <section className="settings-group" aria-label="Sessions">
            <h3>Sessions</h3>
            <Row label="Editor" hint="What “Open in editor” starts for a task.">
                <Picker
                    label="Editor"
                    value={settings.editor}
                    onChange={(editor) => void change(api, { editor })}
                    options={editorChoices(settings.editor)}
                />
            </Row>
            <Row
                label="Name new sessions"
                hint="A small model reads the first message and gives the session a short title."
            >
                <input
                    type="checkbox"
                    aria-label="Name new sessions automatically"
                    checked={settings.auto_title}
                    onChange={(event) => void change(api, { auto_title: event.target.checked })}
                />
            </Row>
        </section>
    );
}

/** The options, with the current value added if a hand-edited settings file holds another. */
function withCurrent(options: [string, string][], current: string): [string, string][] {
    const known = options.some(([value]) => value === current);
    return known ? options : [...options, [current, `${current}%`]];
}

function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
    return (
        <div className="settings-row">
            <div>
                <p className="settings-label">{label}</p>
                {hint && <p className="settings-hint">{hint}</p>}
            </div>
            {children}
        </div>
    );
}

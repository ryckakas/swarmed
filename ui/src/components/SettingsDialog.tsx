import { type ReactNode, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Settings, Theme } from "../api/types";
import { STATUS_ITEM_LABELS, STATUS_ITEMS, withItem } from "../statusline";
import { useStore } from "../store";
import { ApiKeys } from "./ApiKeys";
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
            <StatusLineSettings settings={settings} />
            <ApiKeys />
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
            <Row
                label="HTML views"
                hint="Agents are told they may answer with a small interactive view, such as a sortable table. It runs in a sandboxed frame with no access to the app or the network. Applies to sessions started from now on."
            >
                <input
                    type="checkbox"
                    aria-label="Show HTML views from agents"
                    checked={settings.html_views}
                    onChange={(event) => void change(api, { html_views: event.target.checked })}
                />
            </Row>
        </section>
    );
}

function StatusLineSettings({ settings }: { settings: Settings }) {
    const change = useStore((state) => state.changeSettings);
    const [command, setCommand] = useState(settings.status_command);
    const scripted = settings.status_command.trim() !== "";
    return (
        <section className="settings-group" aria-label="Status line">
            <h3>Status line</h3>
            <p className="settings-hint">
                The line under the message box. Tick what it shows, or let a script of your own
                decide.
            </p>
            <div className="settings-checks" aria-disabled={scripted}>
                {STATUS_ITEMS.map((item) => (
                    <label key={item} className="settings-check">
                        <input
                            type="checkbox"
                            disabled={scripted}
                            checked={settings.status_items.includes(item)}
                            onChange={(event) => {
                                const on = event.target.checked;
                                const items = withItem(settings.status_items, item, on);
                                void change(api, { status_items: items });
                            }}
                        />
                        {STATUS_ITEM_LABELS[item]}
                    </label>
                ))}
            </div>
            <label className="settings-script">
                <span className="settings-label">Custom script</span>
                <span className="settings-hint">
                    Replaces the pieces above. Runs with sh -c in the task's worktree and gets the
                    session, task and project as JSON on standard input. Its first line of output is
                    shown; values separated by | become badges.
                </span>
                <textarea
                    className="key-input"
                    rows={4}
                    placeholder={"jq -r '\"\\(.task.branch) | \\(.session.model)\"'"}
                    spellCheck={false}
                    value={command}
                    onChange={(event) => setCommand(event.target.value)}
                    onBlur={() => void change(api, { status_command: command.trim() })}
                />
            </label>
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

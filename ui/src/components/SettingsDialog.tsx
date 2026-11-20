import { type KeyboardEvent, type ReactNode, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Settings, StatusItem, Style, Supervision, Theme } from "../api/types";
import { PALETTES, STYLES } from "../appearance";
import { keyCaps, LOCAL_HOTKEYS, listedHotkeys } from "../hotkeys";
import { movedItem, STATUS_ITEM_LABELS, STATUS_ITEMS, withItem } from "../statusline";
import { SETTINGS_TABS, type SettingsTab, useStore } from "../store";
import { useWords } from "../useWords";
import { AgentsSettings } from "./AgentsSettings";
import { ApiKeys } from "./ApiKeys";
import { CloseIcon, IconButton } from "./IconButton";
import { McpSettings } from "./McpSettings";
import { editorChoices, supervisionChoices, TEXT_SCALES, THEMES } from "./options";
import { Picker } from "./Picker";
import { SkillsSettings } from "./SkillsSettings";
import { TemplatesSettings } from "./TemplatesSettings";
import { UsageSettings } from "./UsageSettings";

/** The user's preferences. Every change takes effect, and is saved, at once. */
export function SettingsDialog() {
    const open = useStore((state) => state.settingsOpen);
    const settings = useStore((state) => state.settings);
    const showSettings = useStore((state) => state.showSettings);
    const tab = useStore((state) => state.settingsTab);
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
            <header className="settings-head">
                <h2 id="settings-title">Settings</h2>
                <IconButton label="Close settings" onClick={() => showSettings(false)}>
                    <CloseIcon />
                </IconButton>
            </header>
            <div className="settings-main">
                <Tabs tab={tab} />
                {/* Only this part scrolls: the title, the tabs and the way out stay put. */}
                <div
                    className="settings-body"
                    role="tabpanel"
                    id="settings-panel"
                    aria-labelledby={`settings-tab-${tab}`}
                >
                    <Panel tab={tab} settings={settings} />
                </div>
            </div>
            <footer className="settings-foot">
                <button
                    type="button"
                    className="button button-primary"
                    onClick={() => showSettings(false)}
                >
                    Done
                </button>
            </footer>
        </dialog>
    );
}

/** The tabs down the left side. Up and down move between them, as in any list of tabs. */
function Tabs({ tab }: { tab: SettingsTab }) {
    const showSettings = useStore((state) => state.showSettings);
    const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
        const step = { ArrowDown: 1, ArrowUp: -1 }[event.key];
        if (step === undefined) {
            return;
        }
        event.preventDefault();
        const at = SETTINGS_TABS.findIndex(([id]) => id === tab);
        const next = SETTINGS_TABS[(at + step + SETTINGS_TABS.length) % SETTINGS_TABS.length];
        if (next) {
            showSettings(true, next[0]);
            document.getElementById(`settings-tab-${next[0]}`)?.focus();
        }
    };
    return (
        <div
            className="settings-tabs"
            role="tablist"
            aria-label="Settings"
            aria-orientation="vertical"
            onKeyDown={onKeyDown}
        >
            {SETTINGS_TABS.map(([id, label]) => (
                <button
                    key={id}
                    type="button"
                    role="tab"
                    id={`settings-tab-${id}`}
                    className="settings-tab"
                    aria-selected={id === tab}
                    aria-controls="settings-panel"
                    // Only the chosen tab is a Tab stop; the arrows reach the others.
                    tabIndex={id === tab ? 0 : -1}
                    onClick={() => showSettings(true, id)}
                >
                    {label}
                </button>
            ))}
        </div>
    );
}

/** The part of the settings a tab stands for. Only the one on screen is built. */
function Panel({ tab, settings }: { tab: SettingsTab; settings: Settings }) {
    switch (tab) {
        case "appearance":
            return <Appearance settings={settings} />;
        case "sessions":
            return <Sessions settings={settings} />;
        case "rules":
            return <Rules settings={settings} />;
        case "status":
            return <StatusLineSettings settings={settings} />;
        case "agents":
            return <AgentsSettings />;
        case "skills":
            return <SkillsSettings />;
        case "prompts":
            return <TemplatesSettings />;
        case "mcp":
            return <McpSettings />;
        case "keys":
            return <ApiKeys />;
        case "usage":
            return <UsageSettings />;
        case "shortcuts":
            return <Shortcuts />;
    }
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
            <Row label="Palette" hint="Each has a dark and a light form.">
                <div className="palettes" role="radiogroup" aria-label="Palette">
                    {PALETTES.map(([palette, name]) => (
                        // biome-ignore lint/a11y/useSemanticElements: a swatch, drawn in the palette's own colours
                        <button
                            key={palette}
                            type="button"
                            role="radio"
                            className="palette"
                            data-palette={palette}
                            aria-checked={settings.palette === palette}
                            onClick={() => void change(api, { palette })}
                        >
                            <span className="palette-dots" aria-hidden="true">
                                <i className="palette-raised" />
                                <i className="palette-accent" />
                                <i className="palette-ink" />
                            </span>
                            {name}
                        </button>
                    ))}
                </div>
            </Row>
            <Row
                label="Icons and words"
                hint="Swarm draws sessions as eggs and larvae: dormant, mutating, hatched. Works with any palette."
            >
                <Picker
                    label="Icons and words"
                    value={settings.style}
                    onChange={(style) => void change(api, { style: style as Style })}
                    options={STYLES}
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
                label="New sessions start in"
                hint="Auto mode lets the agent decide which actions are safe. It can be changed per session under the message box."
            >
                <Picker
                    label="Supervision of new sessions"
                    value={settings.default_supervision}
                    onChange={(value) =>
                        void change(api, { default_supervision: value as Supervision })
                    }
                    options={supervisionChoices(settings.default_supervision)}
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
                label="Notifications"
                hint="The system tells you when a session needs you, finishes or fails while Swarmed is in the background. The app's icon always counts what waits for you."
            >
                <input
                    type="checkbox"
                    aria-label="Notify when a session needs you, finishes or fails"
                    checked={settings.notifications}
                    onChange={(event) => void change(api, { notifications: event.target.checked })}
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

/** The most the rules may hold, as the core enforces it. */
const MAX_RULES_BYTES = 20_000;

/** What the line beside the Save button says. */
function rulesState(tooLong: boolean, changed: boolean, saved: string): string {
    if (tooLong) {
        return "Too long to be given to every session. Not saved until it is shorter.";
    }
    if (changed) {
        return "Not saved yet";
    }
    return saved === "" ? "No rules" : "Saved";
}

function Rules({ settings }: { settings: Settings }) {
    const change = useStore((state) => state.changeSettings);
    const [rules, setRules] = useState(settings.rules);
    const tooLong = new TextEncoder().encode(rules).length > MAX_RULES_BYTES;
    const changed = rules.trim() !== settings.rules;
    const save = () => {
        if (!tooLong && changed) {
            void change(api, { rules: rules.trim() });
        }
    };
    return (
        <section className="settings-group" aria-label="Rules">
            <h3>Rules</h3>
            <p className="settings-hint">
                What every agent should keep to, whatever the agent or model: how to answer, what to
                avoid, how you like things done. Written in plain words, one rule per line. They are
                given to each session when its agent starts, so a session that is already running
                gets them the next time it is started.
            </p>
            <textarea
                className="key-input rules-input"
                aria-label="Rules for every agent"
                rows={10}
                placeholder={"Keep answers short.\nAsk before adding a dependency."}
                value={rules}
                onChange={(event) => setRules(event.target.value)}
                onBlur={save}
            />
            <div className="rules-foot">
                <button
                    type="button"
                    className="button"
                    disabled={tooLong || !changed}
                    onClick={save}
                >
                    Save rules
                </button>
                <span className="settings-hint" role="status">
                    {rulesState(tooLong, changed, settings.rules)}
                </span>
            </div>
        </section>
    );
}

/**
 * The pieces of the status line: the chosen ones first, in the order they are shown, each with
 * buttons that move it; then the ones left out.
 */
function StatusPieces({ settings, disabled }: { settings: Settings; disabled: boolean }) {
    const change = useStore((state) => state.changeSettings);
    const chosen = settings.status_items;
    const words = useWords();
    const save = (items: StatusItem[]) => void change(api, { status_items: items });
    const rest = STATUS_ITEMS.filter((item) => !chosen.includes(item));
    return (
        <ul className="status-pieces" aria-disabled={disabled}>
            {[...chosen, ...rest].map((item) => {
                const at = chosen.indexOf(item);
                const name = item === "duration" ? words.timeWorked : STATUS_ITEM_LABELS[item];
                return (
                    <li key={item} className="status-piece">
                        <label className="settings-check">
                            <input
                                type="checkbox"
                                disabled={disabled}
                                checked={at !== -1}
                                onChange={(event) =>
                                    save(withItem(chosen, item, event.target.checked))
                                }
                            />
                            {name}
                        </label>
                        {at !== -1 && (
                            <span className="status-piece-moves">
                                <button
                                    type="button"
                                    className="status-piece-move"
                                    aria-label={`Move ${name} earlier`}
                                    title="Earlier"
                                    disabled={disabled || at === 0}
                                    onClick={() => save(movedItem(chosen, item, -1))}
                                >
                                    ↑
                                </button>
                                <button
                                    type="button"
                                    className="status-piece-move"
                                    aria-label={`Move ${name} later`}
                                    title="Later"
                                    disabled={disabled || at === chosen.length - 1}
                                    onClick={() => save(movedItem(chosen, item, 1))}
                                >
                                    ↓
                                </button>
                            </span>
                        )}
                    </li>
                );
            })}
        </ul>
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
                The line under the message box. Tick what it shows and put it in the order you want,
                or let a script of your own decide.
            </p>
            <StatusPieces settings={settings} disabled={scripted} />
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

const SHORTCUT_GROUPS = ["Sessions", "In a session", "Task", "Window"] as const;

/** Every keyboard shortcut, by where it applies. Folded away until asked for: it is a long list. */
function Shortcuts() {
    return (
        <section className="settings-group" aria-label="Keyboard shortcuts">
            <h3>Keyboard shortcuts</h3>
            {SHORTCUT_GROUPS.map((group) => (
                <ShortcutList
                    key={group}
                    title={group}
                    shortcuts={listedHotkeys().filter((hotkey) => hotkey.group === group)}
                />
            ))}
            <ShortcutList title="While typing or in the sidebar" shortcuts={LOCAL_HOTKEYS} />
        </section>
    );
}

interface ShortcutListProps {
    title: string;
    shortcuts: { keys: string; label: string }[];
}

function ShortcutList({ title, shortcuts }: ShortcutListProps) {
    return (
        <>
            <p className="shortcut-group">{title}</p>
            <dl className="shortcuts">
                {shortcuts.map((shortcut) => (
                    <div key={shortcut.keys} className="shortcut">
                        <dt>{shortcut.label}</dt>
                        <dd>
                            {keyCaps(shortcut.keys).map((cap) => (
                                <kbd key={cap}>{cap}</kbd>
                            ))}
                        </dd>
                    </div>
                ))}
            </dl>
        </>
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

import { type KeyboardEvent, useEffect, useState } from "react";
import { api } from "../api";
import type {
    LimitWarnings,
    MathShown,
    Settings,
    StatusItem,
    Style,
    Supervision,
    Theme,
    ToolCalls,
} from "../api/types";
import { LAYOUTS, PALETTES, STYLES } from "../appearance";
import { type PrefixCheck, prefixToSave, problemOf } from "../branchPrefix";
import { daysWithin, removeWarning } from "../putAway";
import { movedItem, STATUS_ITEMS, statusItemName, withItem } from "../statusline";
import { SETTINGS_TABS, type SettingsTab, useStore } from "../store";
import { useWords } from "../useWords";
import { capitalized, type Words } from "../wording";
import { About, lookForUpdate } from "./About";
import { ActionsSettings } from "./ActionsSettings";
import { AgentsSettings } from "./AgentsSettings";
import { ApiKeys } from "./ApiKeys";
import { BrowserSettings } from "./BrowserSettings";
import { Dialog } from "./Dialog";
import { McpSettings } from "./McpSettings";
import {
    editorChoices,
    LIMIT_WARNINGS,
    MATH_SHOWN,
    supervisionChoices,
    TEXT_SCALES,
    THEMES,
    TOOL_CALLS,
    terminalAppChoices,
    WORKER_CHOICES,
} from "./options";
import { Picker } from "./Picker";
import { RoleDefaultsSettings } from "./RoleDefaultsSettings";
import { Row } from "./SettingsRow";
import { ShortcutLists } from "./Shortcuts";
import { SkillsSettings } from "./SkillsSettings";
import { StylesheetSettings } from "./StylesheetSettings";
import { SwarmDictionary } from "./SwarmDictionary";
import { TemplatesSettings } from "./TemplatesSettings";
import { UsageSettings } from "./UsageSettings";

/** The user's preferences. Every change takes effect, and is saved, at once. */
export function SettingsDialog() {
    const words = useWords();
    const open = useStore((state) => state.settingsOpen);
    const settings = useStore((state) => state.settings);
    const showSettings = useStore((state) => state.showSettings);
    const tab = useStore((state) => state.settingsTab);

    if (!(open && settings)) {
        return null;
    }
    // A field such as a key is saved by its own button, so a click beside it could lose what
    // was typed: Esc, the × and Done close it.
    return (
        <Dialog
            title={words.settings}
            size="large"
            fixedHeight={true}
            panes={true}
            className="settings"
            onClose={() => showSettings(false)}
            dismiss="Done"
        >
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
        </Dialog>
    );
}

/** The tabs down the left side. Up and down move between them, as in any list of tabs. */
function Tabs({ tab }: { tab: SettingsTab }) {
    const words = useWords();
    const showSettings = useStore((state) => state.showSettings);
    const style = useStore((state) => state.settings?.style);
    const tabs = shownTabs(style);
    const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
        const step = { ArrowDown: 1, ArrowUp: -1 }[event.key];
        if (step === undefined) {
            return;
        }
        event.preventDefault();
        const at = tabs.findIndex(([id]) => id === tab);
        const next = tabs[(at + step + tabs.length) % tabs.length];
        if (next) {
            showSettings(true, next[0]);
            document.getElementById(`settings-tab-${next[0]}`)?.focus();
        }
    };
    return (
        <div
            className="settings-tabs"
            role="tablist"
            aria-label={words.settings}
            aria-orientation="vertical"
            onKeyDown={onKeyDown}
        >
            {tabs.map(([id, label]) => (
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
                    // The keyboard starts on the chosen tab, where the arrows move from.
                    data-dialog-start={id === tab || undefined}
                    onClick={() => showSettings(true, id)}
                >
                    {tabLabel(id, label, words)}
                </button>
            ))}
        </div>
    );
}

/** The tabs there are in a style: the swarm dictionary only where there are swarm words. */
function shownTabs(style: Style | undefined) {
    return SETTINGS_TABS.filter(([id]) => id !== "dictionary" || style !== "plain");
}

/** A tab's name: the style's own where it has one. */
function tabLabel(id: SettingsTab, label: string, words: Words): string {
    const named: Partial<Record<SettingsTab, string>> = {
        actions: words.actions,
        shortcuts: words.shortcuts,
        rules: capitalized(words.terms.rules),
        agents: capitalized(words.terms.agents),
        skills: capitalized(words.terms.skills),
        prompts: capitalized(words.terms.prompts),
        mcp: capitalized(words.terms.mcpServers),
    };
    return named[id] ?? label;
}

/** The part of the settings a tab stands for. Only the one on screen is built. */
function Panel({ tab, settings }: { tab: SettingsTab; settings: Settings }) {
    switch (tab) {
        case "appearance":
            return <Appearance settings={settings} />;
        case "sessions":
            return (
                <>
                    <Sessions settings={settings} />
                    <RoleDefaultsSettings settings={settings} />
                    <FinishedTasks settings={settings} />
                    <BrowserSettings settings={settings} />
                </>
            );
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
        case "actions":
            return <ActionsSettings />;
        case "terminal":
            return <TerminalSettings />;
        case "mcp":
            return <McpSettings />;
        case "keys":
            return <ApiKeys />;
        case "usage":
            return (
                <>
                    <UsageWarnings settings={settings} />
                    <UsageSettings />
                </>
            );
        case "shortcuts":
            return <Shortcuts />;
        case "dictionary":
            return <SwarmDictionary />;
        case "about":
            return <AboutSection settings={settings} />;
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
                hint="Weak swarm draws sessions as eggs and larvae: dormant, mutating, hatched. True swarm renames the tree too. Works with any palette."
            >
                <Picker
                    label="Icons and words"
                    value={settings.style}
                    onChange={(style) => void change(api, { style: style as Style })}
                    options={STYLES}
                />
            </Row>
            <Row label="Text size" hint="On top of what the layout starts from.">
                <Picker
                    label="Text size"
                    value={String(settings.text_scale)}
                    onChange={(scale) => void change(api, { text_scale: Number(scale) })}
                    options={withCurrent(TEXT_SCALES, String(settings.text_scale))}
                />
            </Row>
            <Row
                label="Layout"
                hint="How large and how far apart things are. Works with any palette."
            >
                <div className="layouts" role="radiogroup" aria-label="Layout">
                    {LAYOUTS.map(([layout, name, what]) => (
                        // biome-ignore lint/a11y/useSemanticElements: a card with a sample, not a bare radio
                        <button
                            key={layout}
                            type="button"
                            role="radio"
                            className="layout-choice"
                            data-layout={layout}
                            aria-checked={settings.layout === layout}
                            onClick={() => void change(api, { layout })}
                        >
                            <span className="layout-sample" aria-hidden="true">
                                Aa
                            </span>
                            <span className="layout-name">{name}</span>
                            <span className="layout-what">{what}</span>
                        </button>
                    ))}
                </div>
            </Row>
            {settings.layout === "custom" && <LayoutEditor />}
            <StylesheetSettings on={settings.user_stylesheet} />
        </section>
    );
}

/** The fields a custom layout may hold, as the hint under its editor. */
const LAYOUT_FIELDS =
    "base (classic or senior), text { xs, sm, md, lg, xl } in pixels, line_height, spacing and icons in percent, " +
    "row_height and control_height in pixels, font, mono_font, and dark and light colours such as dusk, panel, " +
    "raised, line, text, muted, faint, bright, focus, sky, pollen, moss, busy, clay. The sidebar's levels " +
    "(sidebar { repository, task, session, sub_agent }) each take a size in pixels, a weight from 400 to 700, " +
    "and dark and light { colour, background }. Leave out what should stay as it is. An agent can write it for " +
    "you: ask one to change the layout.";

/** The custom layout as the JSON it is. The core checks it on saving and says what is wrong. */
function LayoutEditor() {
    const kept = useStore((state) => state.customLayout);
    const save = useStore((state) => state.saveCustomLayout);
    return (
        <JsonEditor
            label="Custom layout, as JSON"
            hint={LAYOUT_FIELDS}
            kept={kept ?? { base: "classic" }}
            saveLabel="Save layout"
            onSave={(text) => save(api, text)}
        />
    );
}

/** The fields a terminal's configuration may hold, as the hint over its editor. */
const TERMINAL_FIELDS =
    "font, font_size in pixels, line_height in percent, cursor (block, bar or underline), cursor_blink, " +
    "scrollback in lines, bold_is_bright, option_is_meta, copy_on_select, clipboard (programs may copy), gpu, " +
    "minimum_contrast in tenths, and dark and light colours: background, foreground, cursor, selection, black, " +
    "red, green, yellow, blue, magenta, cyan, white and each with bright_ in front. Leave out what should stay " +
    "as it is: the colours then follow the palette. An agent can write it for you: ask one to change the terminal.";

/** How the terminal looks and behaves, as JSON the core checks. */
function TerminalSettings() {
    const kept = useStore((state) => state.terminalConfig);
    const save = useStore((state) => state.saveTerminalConfig);
    return (
        <section className="settings-group" aria-label="Terminal">
            <h3>Terminal</h3>
            <p className="settings-hint">
                The terminal of a session: a shell in the task's worktree. Addresses in its output
                open with a click, and Cmd+F finds in what it printed. Kept in{" "}
                <code>terminal.json</code> in Swarmed's data folder.
            </p>
            <JsonEditor
                label="Terminal, as JSON"
                hint={TERMINAL_FIELDS}
                kept={kept ?? {}}
                saveLabel="Save terminal"
                onSave={(text) => save(api, text)}
            />
        </section>
    );
}

interface JsonEditorProps {
    /** What the editor is, for screen readers. */
    label: string;
    /** Which fields there are. */
    hint: string;
    /** What is kept now. */
    kept: object;
    saveLabel: string;
    /** Keeps the text; resolves to what is wrong with it, or to nothing. */
    onSave(text: string): Promise<string | null>;
}

/** A piece of configuration as the JSON it is. The core checks it on saving and says what is wrong. */
function JsonEditor({ label, hint, kept, saveLabel, onSave }: JsonEditorProps) {
    const [text, setText] = useState<string | null>(null);
    const [problem, setProblem] = useState<string | null>(null);
    const written = JSON.stringify(kept, null, 2);
    const shown = text ?? written;
    const keep = async () => {
        const wrong = await onSave(shown);
        setProblem(wrong);
        // Once kept, the editor shows it as the core wrote it down.
        setText(wrong === null ? null : shown);
    };
    const revert = () => {
        setText(null);
        setProblem(null);
    };
    return (
        <div className="layout-editor">
            <p className="settings-hint">{hint}</p>
            <textarea
                className="layout-json"
                aria-label={label}
                spellCheck={false}
                value={shown}
                onChange={(event) => setText(event.target.value)}
            />
            <div className="layout-foot">
                <p className="layout-problem" role="alert">
                    {problem}
                </p>
                <button
                    type="button"
                    className="button"
                    disabled={shown === written}
                    onClick={revert}
                >
                    Revert
                </button>
                <button
                    type="button"
                    className="button button-primary"
                    disabled={shown === written}
                    onClick={keep}
                >
                    {saveLabel}
                </button>
            </div>
        </div>
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
                label="Terminal application"
                hint="What “Open in Terminal” starts in a task's worktree."
            >
                <Picker
                    label="Terminal application"
                    value={settings.terminal_app}
                    onChange={(terminal_app) => void change(api, { terminal_app })}
                    options={terminalAppChoices(settings.terminal_app)}
                />
            </Row>
            <BranchPrefix kept={settings.branch_prefix} />
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
                label="Workers per session"
                hint="How many open sessions may work for one session, however its agent started them. That agent starts no more until one is closed, so a runaway one cannot fill the machine. Agents cannot change this."
            >
                <Picker
                    label="Workers per session"
                    value={String(settings.max_workers)}
                    onChange={(count) => void change(api, { max_workers: Number(count) })}
                    options={WORKER_CHOICES}
                />
            </Row>
            <Row
                label="Tool calls"
                hint="How what the agent runs shows in a conversation. Permission requests, questions, plans and views always show."
            >
                <Picker
                    label="Tool calls"
                    value={settings.tool_calls}
                    onChange={(value) => void change(api, { tool_calls: value as ToolCalls })}
                    options={TOOL_CALLS}
                />
            </Row>
            <Row
                label="Turn end lines"
                hint="A line where each turn that showed something ends, with how long it took and what was spent so far. A turn that failed or was stopped keeps its line either way, and the status line's Cost piece always shows the cost."
            >
                <input
                    type="checkbox"
                    aria-label="Draw a line where each turn ends"
                    checked={settings.turn_end_lines}
                    onChange={(event) => void change(api, { turn_end_lines: event.target.checked })}
                />
            </Row>
            <Row
                label="Math"
                hint="Which formulas in agents' replies are drawn as math: $$…$$, \(…\), \[…\] and math blocks, and with Also single $, $…$ where it reads as math. Prices and shell variables such as $5 and $HOME stay text, code is never drawn, and copying a formula gives its TeX."
            >
                <Picker
                    label="Math"
                    value={settings.math}
                    onChange={(value) => void change(api, { math: value as MathShown })}
                    options={MATH_SHOWN}
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
            <SnoozeTimes settings={settings} />
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

/** The hours of a day, as the snooze times are chosen from. */
const HOURS: [string, string][] = Array.from({ length: 24 }, (_, hour) => [
    String(hour),
    `${String(hour).padStart(2, "0")}:00`,
]);

/** The hours a snooze until the morning or this evening wakes at. */
function SnoozeTimes({ settings }: { settings: Settings }) {
    const change = useStore((state) => state.changeSettings);
    return (
        <Row
            label="Snooze times"
            hint="When a session snoozed until the morning or this evening comes back. Next Monday uses the morning's hour too."
        >
            <span className="snooze-hours">
                <span className="snooze-hour">
                    Morning
                    <Picker
                        label="Snooze until the morning: the hour"
                        value={String(settings.snooze_morning_hour)}
                        onChange={(hour) => void change(api, { snooze_morning_hour: Number(hour) })}
                        options={HOURS}
                    />
                </span>
                <span className="snooze-hour">
                    Evening
                    <Picker
                        label="Snooze until this evening: the hour"
                        value={String(settings.snooze_evening_hour)}
                        onChange={(hour) => void change(api, { snooze_evening_hour: Number(hour) })}
                        options={HOURS}
                    />
                </span>
            </span>
        </Row>
    );
}

/**
 * What new tasks' branches start with. The branch it gives is shown as it is typed, or why git
 * would not take it; it is kept when the field is left or Enter is pressed.
 */
function BranchPrefix({ kept }: { kept: string }) {
    const save = useStore((state) => state.saveBranchPrefix);
    // What is typed and not kept yet; `null` shows what is kept.
    const [typed, setTyped] = useState<string | null>(null);
    const [check, setCheck] = useState<PrefixCheck | null>(null);
    const shown = typed ?? kept;
    useEffect(() => {
        // Only the answer for what the field shows now: an earlier one may come later.
        let current = true;
        const heard = (said: PrefixCheck) => {
            if (current) {
                setCheck(said);
            }
        };
        api.branchExample(shown).then(
            (branch) => heard({ prefix: shown, branch }),
            (error: unknown) => heard({ prefix: shown, problem: problemOf(error) }),
        );
        return () => {
            current = false;
        };
    }, [shown]);
    const keep = async () => {
        const prefix = prefixToSave(typed, kept, check);
        if (prefix === null) {
            return;
        }
        const problem = await save(api, prefix);
        if (problem === null) {
            setTyped(null);
        } else {
            setCheck({ prefix, problem });
        }
    };
    const refused = check !== null && "problem" in check;
    return (
        <Row
            label="Branch prefix"
            hint="What a new task's branch starts with, before a / and the task's name. Empty, it is your login. A repository whose config sets a branch_pattern of its own keeps it."
        >
            <div className="branch-prefix">
                <input
                    type="text"
                    className="key-input branch-prefix-input"
                    aria-label="Branch prefix"
                    placeholder="Your login"
                    spellCheck={false}
                    autoCapitalize="off"
                    autoCorrect="off"
                    value={shown}
                    onChange={(event) => setTyped(event.target.value)}
                    onBlur={() => void keep()}
                    onKeyDown={(event) => {
                        if (event.key === "Enter") {
                            void keep();
                        }
                    }}
                />
                <p className="branch-prefix-said" role="status" data-refused={refused}>
                    {check !== null && "branch" in check ? (
                        <>
                            For example <code>{check.branch}</code>
                        </>
                    ) : (
                        check?.problem
                    )}
                </p>
            </div>
        </Row>
    );
}

/**
 * Which levels of the account's usage the notice at the top of the window says. With the
 * usage figures, where someone looking for less of them would look.
 */
function UsageWarnings({ settings }: { settings: Settings }) {
    const change = useStore((state) => state.changeSettings);
    return (
        <section className="settings-group" aria-label="Claude Code's usage limits">
            <h3>Claude Code's usage limits</h3>
            <Row
                label="Usage warnings"
                hint="Which levels of your 5-hour and weekly limits the notice at the top of the window says, in Claude Code sessions: 50%, 75% and 90% used, a limit reached with extra usage paying, and a limit that stops requests (Frequent); only from 90% (Rare); or only requests stopped (None). Dismissed, it stays away in every session until a higher level, until its window resets, or until a limit first reported since reaches a level. The coins at the top of a session always show how much is used."
            >
                <Picker
                    label="Usage warnings"
                    value={settings.limit_warnings}
                    onChange={(value) =>
                        void change(api, { limit_warnings: value as LimitWarnings })
                    }
                    options={LIMIT_WARNINGS}
                />
            </Row>
            <Row
                label="Continue after a limit"
                hint="A session a usage limit stopped goes on by itself when the limit resets: Swarmed sends it “Continue where you stopped.” then, as if you had typed it. Without this, the line where the limit stopped it offers that each time. Writing to the session, Stop, or the clock beside it cancels it. Swarmed has to be open at the reset; one that was not sends it when it next opens. Nothing asks Claude Code for its usage to do this."
            >
                <input
                    type="checkbox"
                    aria-label="Continue a session by itself when the usage limit that stopped it resets"
                    checked={settings.continue_after_limit}
                    onChange={(event) =>
                        void change(api, { continue_after_limit: event.target.checked })
                    }
                />
            </Row>
        </section>
    );
}

/**
 * Putting finished tasks away by themselves: nothing is, unless a rule is turned on. Each rule
 * has its own number of days, and what putting away does to the sessions is a choice apart, as
 * is removing a task once it has been put away for some days.
 */
function FinishedTasks({ settings }: { settings: Settings }) {
    const words = useWords();
    const change = useStore((state) => state.changeSettings);
    const tasks = words.terms.tasks;
    return (
        <section className="settings-group" aria-label={`Finished ${tasks}`}>
            <h3>Finished {tasks}</h3>
            <Row
                label="Put away once merged"
                hint={`A ${words.terms.task} whose ${words.terms.pullRequest} merged is put away this many days after the merge: its ${words.terms.agent}s and its stack are stopped and it is listed under Done, with nothing deleted. Never while one of its sessions is at work or waits for you, but work in it after the merge does not put that off. A ${words.terms.task} you bring back stays.`}
            >
                <RuleDays
                    label={`Put away ${tasks} whose ${words.terms.pullRequest} merged`}
                    on={settings.put_away_merged}
                    days={settings.put_away_merged_days}
                    least={0}
                    onOn={(on) => void change(api, { put_away_merged: on })}
                    onDays={(days) => void change(api, { put_away_merged_days: days })}
                />
            </Row>
            <Row
                label="Put away when idle"
                hint={`A ${words.terms.task} none of whose sessions did anything for this many days is put away the same way. A terminal or a running stack does not count as doing something.`}
            >
                <RuleDays
                    label={`Put away idle ${tasks}`}
                    on={settings.put_away_idle}
                    days={settings.put_away_idle_days}
                    least={1}
                    onOn={(on) => void change(api, { put_away_idle: on })}
                    onDays={(days) => void change(api, { put_away_idle_days: days })}
                />
            </Row>
            <Row
                label="Remove when put away"
                hint={`A ${words.terms.task} put away this many days ago is removed as Remove… does: its ${words.terms.worktree}, its sessions and your notes on it go, and its local ${words.terms.branch} too unless ${words.terms.commits} on it exist nowhere else. 0 never removes. Never while one of its sessions is at work.`}
            >
                <RemoveDays settings={settings} />
            </Row>
            <Row
                label="Close sessions too"
                hint={`Putting a ${words.terms.task} away, by hand or by a rule, also closes its sessions. They keep their conversations and can be reopened. Without this they stay open, with their ${words.terms.agent}s stopped.`}
            >
                <input
                    type="checkbox"
                    aria-label={`Close a ${words.terms.task}'s sessions when it is put away`}
                    checked={settings.put_away_closes_sessions}
                    onChange={(event) =>
                        void change(api, { put_away_closes_sessions: event.target.checked })
                    }
                />
            </Row>
        </section>
    );
}

interface RuleDaysProps {
    label: string;
    on: boolean;
    days: number;
    /** The fewest days the rule takes. */
    least: number;
    onOn(on: boolean): void;
    onDays(days: number): void;
}

/** A rule turned on or off, and after how many days. */
function RuleDays({ label, on, days, least, onOn, onDays }: RuleDaysProps) {
    return (
        <span className="rule-days">
            <input
                type="checkbox"
                aria-label={label}
                checked={on}
                onChange={(event) => onOn(event.target.checked)}
            />
            <DaysInput
                label={`${label}: after how many days`}
                days={days}
                least={least}
                onDays={onDays}
            />
            <span className="rule-days-unit">{days === 1 ? "day" : "days"}</span>
        </span>
    );
}

/**
 * After how many days a task put away is removed by itself. Once it is removed at all, the
 * warning that its uncommitted changes go with it shows under the days: it appears when the
 * days are kept, never under the pointer.
 */
function RemoveDays({ settings }: { settings: Settings }) {
    const words = useWords();
    const change = useStore((state) => state.changeSettings);
    const days = settings.put_away_remove_days;
    return (
        <div className="remove-days">
            <span className="rule-days">
                <DaysInput
                    label={`Remove ${words.terms.tasks} put away: after how many days, 0 for never`}
                    days={days}
                    least={0}
                    onDays={(kept) => void change(api, { put_away_remove_days: kept })}
                />
                <span className="rule-days-unit">{days === 1 ? "day" : "days"}</span>
            </span>
            <p className="remove-days-warning" role="status">
                {removeWarning(days, words.terms.tasks)}
            </p>
        </div>
    );
}

interface DaysInputProps {
    label: string;
    days: number;
    /** The fewest days it takes. */
    least: number;
    onDays(days: number): void;
}

/** A number of days, up to 365, kept once typed: on leaving the field, or with Enter. */
function DaysInput({ label, days, least, onDays }: DaysInputProps) {
    const [typed, setTyped] = useState(String(days));
    const keep = () => {
        const kept = daysWithin(typed, least);
        setTyped(String(kept));
        if (kept !== days) {
            onDays(kept);
        }
    };
    return (
        <input
            className="key-input mono rule-days-count"
            aria-label={label}
            type="number"
            inputMode="numeric"
            min={least}
            max={365}
            step={1}
            value={typed}
            onChange={(event) => setTyped(event.target.value)}
            onBlur={keep}
            onKeyDown={(event) => event.key === "Enter" && keep()}
        />
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
    const words = useWords();
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
        <section className="settings-group" aria-label={capitalized(words.terms.rules)}>
            <h3>{capitalized(words.terms.rules)}</h3>
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
                const name = statusItemName(item, words);
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

/** Every keyboard shortcut, by where it applies. */
function Shortcuts() {
    const words = useWords();
    return (
        <section className="settings-group" aria-label={words.shortcuts}>
            <h3>{words.shortcuts}</h3>
            <ShortcutLists />
        </section>
    );
}

function AboutSection({ settings }: { settings: Settings }) {
    const change = useStore((state) => state.changeSettings);
    const words = useWords();
    return (
        <section className="settings-group" aria-label="About">
            <h3>About</h3>
            <About />
            <Row
                label={words.update.automatic}
                hint="Swarmed looks for a new version at launch and every few hours, downloads it, and installs it when you quit. Off, it looks only when this view or About opens, and installs only when you ask."
            >
                <input
                    type="checkbox"
                    aria-label={words.update.automatic}
                    checked={settings.updates === "automatic"}
                    onChange={(event) =>
                        // A version held ready is put away or downloaded again by the look.
                        void change(api, {
                            updates: event.target.checked ? "automatic" : "when_asked",
                        }).then(lookForUpdate)
                    }
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

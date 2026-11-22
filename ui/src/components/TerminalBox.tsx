import "@xterm/xterm/css/xterm.css";
import { ClipboardAddon } from "@xterm/addon-clipboard";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import { type RefObject, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Unsubscribe } from "../api/client";
import type { TerminalConfig, TerminalOpened, TerminalOutput } from "../api/types";
import { useStore } from "../store";
import { schemeOf, terminalExtras, terminalOptions, terminalTheme } from "../terminalLook";
import { endedEarly, heard, redrawn } from "../terminalOutput";

/**
 * Turns a colour as the stylesheet writes it, `var(--clay)` or a mix of two, into the `#rrggbb`
 * a terminal takes: the page works it out on an element of its own, and a canvas reads it.
 */
function colourReader(element: HTMLElement) {
    const probe = element.appendChild(document.createElement("span"));
    probe.hidden = true;
    const canvas = document.createElement("canvas").getContext("2d", { willReadFrequently: true });
    const resolved = (css: string) => {
        probe.style.color = css;
        const computed = getComputedStyle(probe).color;
        if (!canvas) {
            return computed;
        }
        canvas.clearRect(0, 0, 1, 1);
        canvas.fillStyle = computed;
        canvas.fillRect(0, 0, 1, 1);
        const [red = 0, green = 0, blue = 0] = canvas.getImageData(0, 0, 1, 1).data;
        return `#${[red, green, blue].map((part) => part.toString(16).padStart(2, "0")).join("")}`;
    };
    return { resolved, done: () => probe.remove() };
}

/** Opens an address from the terminal in the user's browser; one that is no web link is refused. */
function openLink(uri: string) {
    api.openLink(uri).catch(useStore.getState().report);
}

/** Whether the system is dark right now, kept up to date. */
export function useSystemDark(): boolean {
    const [dark, setDark] = useState(() => matchMedia("(prefers-color-scheme: dark)").matches);
    useEffect(() => {
        const query = matchMedia("(prefers-color-scheme: dark)");
        const heard = () => setDark(query.matches);
        query.addEventListener("change", heard);
        return () => query.removeEventListener("change", heard);
    }, []);
    return dark;
}

/**
 * A terminal drawn as the configuration and the app's colours say, not yet tied to a shell.
 * One that only `shows` what was printed elsewhere lets nothing in it write the clipboard:
 * that is for programs the user runs, not for whatever an agent's command printed.
 */
export function built(
    element: HTMLElement,
    config: TerminalConfig | null,
    scheme: "dark" | "light",
    shows = false,
) {
    const style = getComputedStyle(element);
    const colours = colourReader(element);
    const terminal = new Terminal({
        ...terminalOptions(config, {
            fontFamily: style.fontFamily,
            fontSize: Number.parseFloat(style.fontSize),
        }),
        theme: terminalTheme(config, scheme, colours.resolved),
        // Links a program marks as such (OSC 8), such as the files `ls --hyperlink` lists.
        linkHandler: { activate: (_event, uri) => openLink(uri) },
        // The widths of emoji and other wide characters are an addon's, which asks for this.
        allowProposedApi: true,
    });
    colours.done();
    const fit = new FitAddon();
    const search = new SearchAddon();
    terminal.loadAddon(fit);
    terminal.loadAddon(search);
    // Addresses in plain output: underlined under the pointer, opened by a click.
    terminal.loadAddon(new WebLinksAddon((_event, uri) => openLink(uri)));
    terminal.loadAddon(new Unicode11Addon());
    terminal.unicode.activeVersion = "11";
    terminal.open(element);
    extras(terminal, config, !shows);
    fit.fit();
    return { terminal, fit, search };
}

/** What a configuration can switch off: the clipboard, drawing on the graphics card, copying. */
function extras(terminal: Terminal, config: TerminalConfig | null, writes: boolean) {
    const { clipboard, gpu, copyOnSelect } = terminalExtras(config);
    const copy = (text: string) => void navigator.clipboard.writeText(text).catch(() => undefined);
    if (clipboard && writes) {
        // Programs may put text on the clipboard (OSC 52). What is on it is never told to them.
        const writeOnly = {
            readText: () => "",
            writeText: (_which: unknown, text: string) => copy(text),
        };
        terminal.loadAddon(new ClipboardAddon(undefined, writeOnly));
    }
    if (copyOnSelect) {
        terminal.onSelectionChange(() => terminal.hasSelection() && copy(terminal.getSelection()));
    }
    if (gpu) {
        drawOnGpu(terminal);
    }
}

/** Draws on the graphics card where there is one to draw on; otherwise as before. */
function drawOnGpu(terminal: Terminal) {
    try {
        const webgl = new WebglAddon();
        // The system may take the context away, after sleep for one; the plain renderer takes over.
        webgl.onContextLoss(() => webgl.dispose());
        terminal.loadAddon(webgl);
    } catch {
        // No WebGL here: the terminal is drawn without it.
    }
}

/**
 * Types a command an action asked for, as if the user had, once the shell is up. The command is
 * taken from the store, not read from this render: an effect may run more than once for one
 * render, and each run would type it again.
 */
function useTypedCommand(sessionId: number, opened: RefObject<Promise<unknown>>, shell: boolean) {
    const waiting = useStore((state) => state.terminalCommand);
    useEffect(() => {
        // An action's command is the shell's to run, never a Claude Code's that is shown.
        if (waiting === null || !shell) {
            return;
        }
        const { report, takeTerminalCommand } = useStore.getState();
        const text = takeTerminalCommand(sessionId);
        if (text === null) {
            return;
        }
        void opened.current.then(() => api.terminalWrite(sessionId, `${text}\n`)).catch(report);
    }, [waiting, sessionId, opened, shell]);
}

/** What a terminal is tied to: the session's shell, or the Claude Code it opened on a command. */
interface Line {
    sessionId: number;
    listen(listener: (output: TerminalOutput) => void): Promise<Unsubscribe>;
    open(cols: number, rows: number): Promise<TerminalOpened>;
    write(data: string): Promise<void>;
    resize(cols: number, rows: number): Promise<void>;
    /** What follows when what runs there has ended, or could not be opened. */
    gone(): void;
}

/** The session's shell, or with `claude`, Claude Code opened on that command's screen. */
function lineOf(sessionId: number, claude: string | undefined): Line {
    if (claude === undefined) {
        return {
            sessionId,
            listen: (listener) => api.onTerminalOutput(listener),
            open: (cols, rows) => api.terminalOpen(sessionId, cols, rows),
            write: (data) => api.terminalWrite(sessionId, data),
            resize: (cols, rows) => api.terminalResize(sessionId, cols, rows),
            // A shell that exits is started again by what is typed next.
            gone: () => undefined,
        };
    }
    return {
        sessionId,
        listen: (listener) => api.onClaudeTerminalOutput(listener),
        open: (cols, rows) => api.claudeTerminalOpen(sessionId, claude, cols, rows),
        write: (data) => api.claudeTerminalWrite(sessionId, data),
        resize: (cols, rows) => api.claudeTerminalResize(sessionId, cols, rows),
        gone: () => useStore.getState().leaveClaudeTerminal(sessionId),
    };
}

function isFind(event: KeyboardEvent): boolean {
    return event.type === "keydown" && event.metaKey && !event.shiftKey && event.key === "f";
}

/**
 * Ties a terminal to its line: what runs there prints is drawn, what is typed goes to it, and it
 * is told the terminal's size. Returns what unties them and takes the terminal down.
 */
function tied(
    terminal: Terminal,
    fit: FitAddon,
    element: HTMLElement,
    line: Line,
    opened: RefObject<Promise<unknown>>,
): () => void {
    const { report } = useStore.getState();
    let closed = false;
    // Output that arrives while the shell's earlier output is still being fetched waits
    // here, so nothing is drawn twice or out of order.
    const early: TerminalOutput[] = [];
    // What was fetched, once it is drawn: which run it is and where its output ended. A piece
    // from before that can still arrive afterwards, since it travels apart from the answer,
    // and is in it already; so can one of a run this one took the place of.
    let shown: TerminalOpened | null = null;
    const listening = line.listen((output) => {
        if (output.session_id !== line.sessionId) {
            return;
        }
        if (shown === null) {
            early.push(output);
            return;
        }
        const what = heard(output, shown);
        if (what === "ended") {
            line.gone();
        } else if (what === "printed") {
            terminal.write(output.data);
        }
    });
    // Asked for only once this is listening: what is printed in between would be in neither
    // the replay nor anything heard.
    opened.current = listening
        .then(() => line.open(terminal.cols, terminal.rows))
        .then((opened) => {
            terminal.write(redrawn(opened, early));
            shown = opened;
            terminal.focus();
            if (endedEarly(opened, early)) {
                line.gone();
            }
        })
        .catch((error: unknown) => {
            report(error);
            line.gone();
        });

    const typing = terminal.onData((data) => {
        line.write(data).catch(report);
    });
    const resizing = terminal.onResize(({ cols, rows }) => {
        // What ran there may be gone; the next thing typed says so.
        line.resize(cols, rows).catch(() => undefined);
    });
    const observer = new ResizeObserver(() => !closed && fit.fit());
    observer.observe(element);

    return () => {
        closed = true;
        observer.disconnect();
        typing.dispose();
        resizing.dispose();
        void listening.then((stop) => stop());
        terminal.dispose();
    };
}

/** The line that finds text in the terminal's output: Enter for the next, Shift+Enter the one before. */
function FindBar({ search, onClose }: { search: SearchAddon; onClose(): void }) {
    const [term, setTerm] = useState("");
    const [missing, setMissing] = useState(false);
    const find = (text: string, back: boolean) => {
        const found = back
            ? search.findPrevious(text)
            : search.findNext(text, { incremental: !back });
        setMissing(text !== "" && !found);
    };
    return (
        <div className="terminal-find">
            <input
                // biome-ignore lint/a11y/noAutofocus: opened by the shortcut to be typed into at once
                autoFocus
                className="key-input"
                aria-label="Find in the terminal"
                placeholder="Find"
                spellCheck={false}
                data-missing={missing}
                value={term}
                onChange={(event) => {
                    setTerm(event.target.value);
                    find(event.target.value, false);
                }}
                onKeyDown={(event) => {
                    if (event.key === "Enter") {
                        find(term, event.shiftKey);
                    }
                    if (event.key === "Escape") {
                        event.stopPropagation();
                        onClose();
                    }
                }}
            />
            <button type="button" className="icon-button" aria-label="Close find" onClick={onClose}>
                ×
            </button>
        </div>
    );
}

/**
 * A terminal in the task's worktree, in the place of the message box. The shell belongs to the
 * session and keeps running while this is off screen; coming back redraws what it printed. With
 * `claude`, it shows Claude Code opened on that command's screen instead, apart from the shell.
 */
export function TerminalBox({
    sessionId,
    claude,
}: {
    sessionId: number;
    claude: string | undefined;
}) {
    const holder = useRef<HTMLDivElement>(null);
    const settings = useStore((state) => state.settings);
    const customLayout = useStore((state) => state.customLayout);
    const config = useStore((state) => state.terminalConfig);
    const scheme = schemeOf(settings?.theme, useSystemDark());
    // Settles once the shell is there to be typed into.
    const opened = useRef<Promise<unknown>>(Promise.resolve());
    const [found, setFound] = useState<{
        search: SearchAddon;
        focus(): void;
        finding: boolean;
    } | null>(null);

    // biome-ignore lint/correctness/useExhaustiveDependencies: rebuilt when the palette, layout or text size changes
    useEffect(() => {
        const element = holder.current;
        if (!element) {
            return;
        }
        const { terminal, fit, search } = built(element, config, scheme);
        setFound({ search, focus: () => terminal.focus(), finding: false });
        // Cmd+F finds in what the terminal printed, while it is the terminal that is typed into.
        terminal.attachCustomKeyEventHandler((event) => {
            const finds = isFind(event);
            if (finds) {
                setFound((held) => held && { ...held, finding: true });
            }
            return !finds;
        });
        return tied(terminal, fit, element, lineOf(sessionId, claude), opened);
    }, [
        sessionId,
        claude,
        scheme,
        settings?.palette,
        settings?.layout,
        settings?.text_scale,
        customLayout,
        config,
    ]);
    // After the effect above, so that on first showing there is a shell being opened to wait for.
    useTypedCommand(sessionId, opened, claude === undefined);

    const closeFind = () => {
        found?.search.clearDecorations();
        found?.focus();
        setFound((held) => held && { ...held, finding: false });
    };
    return (
        <div className="terminal-holder">
            {found?.finding && <FindBar search={found.search} onClose={closeFind} />}
            <div className="terminal-box" ref={holder} />
        </div>
    );
}

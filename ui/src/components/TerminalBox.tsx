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
import type { TerminalConfig, TerminalOutput } from "../api/types";
import { useStore } from "../store";
import { schemeOf, terminalExtras, terminalOptions, terminalTheme } from "../terminalLook";
import { comesAfter, redrawn } from "../terminalOutput";

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
function useSystemDark(): boolean {
    const [dark, setDark] = useState(() => matchMedia("(prefers-color-scheme: dark)").matches);
    useEffect(() => {
        const query = matchMedia("(prefers-color-scheme: dark)");
        const heard = () => setDark(query.matches);
        query.addEventListener("change", heard);
        return () => query.removeEventListener("change", heard);
    }, []);
    return dark;
}

/** A terminal drawn as the configuration and the app's colours say, not yet tied to a shell. */
function built(element: HTMLElement, config: TerminalConfig | null, scheme: "dark" | "light") {
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
    extras(terminal, config);
    fit.fit();
    return { terminal, fit, search };
}

/** What a configuration can switch off: the clipboard, drawing on the graphics card, copying. */
function extras(terminal: Terminal, config: TerminalConfig | null) {
    const { clipboard, gpu, copyOnSelect } = terminalExtras(config);
    const copy = (text: string) => void navigator.clipboard.writeText(text).catch(() => undefined);
    if (clipboard) {
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
function useTypedCommand(sessionId: number, opened: RefObject<Promise<unknown>>) {
    const waiting = useStore((state) => state.terminalCommand);
    useEffect(() => {
        if (waiting === null) {
            return;
        }
        const { report, takeTerminalCommand } = useStore.getState();
        const text = takeTerminalCommand(sessionId);
        if (text === null) {
            return;
        }
        void opened.current.then(() => api.terminalWrite(sessionId, `${text}\n`)).catch(report);
    }, [waiting, sessionId, opened]);
}

function isFind(event: KeyboardEvent): boolean {
    return event.type === "keydown" && event.metaKey && !event.shiftKey && event.key === "f";
}

/**
 * Ties a terminal to the session's shell: what the shell prints is drawn, what is typed goes to
 * it, and it is told the terminal's size. Returns what unties them and takes the terminal down.
 */
function tied(
    terminal: Terminal,
    fit: FitAddon,
    element: HTMLElement,
    sessionId: number,
    opened: RefObject<Promise<unknown>>,
): () => void {
    const { report } = useStore.getState();
    let closed = false;
    // Output that arrives while the shell's earlier output is still being fetched waits
    // here, so nothing is drawn twice or out of order.
    const early: TerminalOutput[] = [];
    // Where the fetched output ended, once it is drawn. A piece from before that can still
    // arrive afterwards, since it travels apart from the answer, and is in it already.
    let end: number | null = null;
    const listening = api.onTerminalOutput((output) => {
        if (output.session_id !== sessionId) {
            return;
        }
        if (end === null) {
            early.push(output);
        } else if (comesAfter(output, end)) {
            terminal.write(output.data);
        }
    });
    // Asked for only once this is listening: what is printed in between would be in neither
    // the replay nor anything heard.
    opened.current = listening
        .then(() => api.terminalOpen(sessionId, terminal.cols, terminal.rows))
        .then((shell) => {
            terminal.write(redrawn(shell, early));
            end = shell.end;
            terminal.focus();
        })
        .catch(report);

    const typing = terminal.onData((data) => {
        api.terminalWrite(sessionId, data).catch(report);
    });
    const resizing = terminal.onResize(({ cols, rows }) => {
        // The shell may be gone; the next thing typed says so.
        api.terminalResize(sessionId, cols, rows).catch(() => undefined);
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
 * session and keeps running while this is off screen; coming back redraws what it printed.
 */
export function TerminalBox({ sessionId }: { sessionId: number }) {
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
        return tied(terminal, fit, element, sessionId, opened);
    }, [
        sessionId,
        scheme,
        settings?.palette,
        settings?.layout,
        settings?.text_scale,
        customLayout,
        config,
    ]);
    // After the effect above, so that on first showing there is a shell being opened to wait for.
    useTypedCommand(sessionId, opened);

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

import "@xterm/xterm/css/xterm.css";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal } from "@xterm/xterm";
import { type RefObject, useEffect, useRef } from "react";
import { api } from "../api";
import { useStore } from "../store";

/** The colours of the terminal, read from the page so it follows the theme. */
function theme(element: HTMLElement) {
    const style = getComputedStyle(element);
    const token = (name: string) => style.getPropertyValue(name).trim();
    return {
        background: token("--panel"),
        foreground: token("--text"),
        cursor: token("--text"),
        selectionBackground: token("--line"),
        green: token("--moss"),
        red: token("--clay"),
        yellow: token("--pollen"),
    };
}

/** Opens an address from the terminal in the user's browser; one that is no web link is refused. */
function openLink(uri: string) {
    api.openLink(uri).catch(useStore.getState().report);
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

/**
 * A terminal in the task's worktree, in the place of the message box. The shell belongs to the
 * session and keeps running while this is off screen; coming back redraws what it printed.
 */
export function TerminalBox({ sessionId }: { sessionId: number }) {
    const holder = useRef<HTMLDivElement>(null);
    const settings = useStore((state) => state.settings);
    const customLayout = useStore((state) => state.customLayout);
    // Settles once the shell is there to be typed into.
    const opened = useRef<Promise<unknown>>(Promise.resolve());

    // biome-ignore lint/correctness/useExhaustiveDependencies: rebuilt when the theme, layout or text size changes
    useEffect(() => {
        const element = holder.current;
        if (!element) {
            return;
        }
        const { report } = useStore.getState();
        const terminal = new Terminal({
            cursorBlink: true,
            fontFamily: getComputedStyle(element).fontFamily,
            fontSize: Number.parseFloat(getComputedStyle(element).fontSize),
            theme: theme(element),
            // A build's whole output stays within reach of the wheel.
            scrollback: 10_000,
            // Links a program marks as such (OSC 8), such as the files `ls --hyperlink` lists.
            linkHandler: { activate: (_event, uri) => openLink(uri) },
        });
        const fit = new FitAddon();
        terminal.loadAddon(fit);
        // Addresses in plain output: underlined under the pointer, opened by a click.
        terminal.loadAddon(new WebLinksAddon((_event, uri) => openLink(uri)));
        terminal.open(element);
        fit.fit();

        let closed = false;
        // Output that arrives while the shell's earlier output is still being fetched waits
        // here, so nothing is drawn twice or out of order.
        let early: string[] | null = [];
        const listening = api.onTerminalOutput((output) => {
            if (output.session_id !== sessionId) {
                return;
            }
            if (early) {
                early.push(output.data);
            } else {
                terminal.write(output.data);
            }
        });
        opened.current = api
            .terminalOpen(sessionId, terminal.cols, terminal.rows)
            .then((replay) => {
                // A replay already holds whatever arrived early; a fresh shell has only that.
                terminal.write(replay === "" ? (early ?? []).join("") : replay);
                early = null;
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
    }, [sessionId, settings?.theme, settings?.layout, settings?.text_scale, customLayout]);
    // After the effect above, so that on first showing there is a shell being opened to wait for.
    useTypedCommand(sessionId, opened);

    return <div className="terminal-box" ref={holder} />;
}

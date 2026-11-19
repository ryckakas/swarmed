import "@xterm/xterm/css/xterm.css";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import { useEffect, useRef } from "react";
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

/**
 * A terminal in the task's worktree, in the place of the message box. The shell belongs to the
 * session and keeps running while this is off screen; coming back redraws what it printed.
 */
export function TerminalBox({ sessionId }: { sessionId: number }) {
    const holder = useRef<HTMLDivElement>(null);
    const settings = useStore((state) => state.settings);

    // biome-ignore lint/correctness/useExhaustiveDependencies: rebuilt when the theme or text size changes
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
        });
        const fit = new FitAddon();
        terminal.loadAddon(fit);
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
        api.terminalOpen(sessionId, terminal.cols, terminal.rows)
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
    }, [sessionId, settings?.theme, settings?.text_scale]);

    return <div className="terminal-box" ref={holder} />;
}

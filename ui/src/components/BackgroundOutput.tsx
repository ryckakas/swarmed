import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { BackgroundTask, TerminalConfig } from "../api/types";
import { type OutputNote, outputNote, outputStep } from "../backgroundOutput";
import { useStore } from "../store";
import { schemeOf } from "../terminalLook";
import { built, useSystemDark } from "./TerminalBox";

/** How often the output of a command still running is read again, while it is shown. */
const FOLLOW_MS = 2000;

/** A terminal that only shows: nothing typed into it goes anywhere. */
function readOnly(element: HTMLElement, config: TerminalConfig | null, scheme: "dark" | "light") {
    const shown = built(element, config, scheme, true);
    shown.terminal.options.disableStdin = true;
    shown.terminal.options.cursorBlink = false;
    shown.terminal.options.convertEol = true;
    return shown;
}

/**
 * Shows in `element` what `task` printed, and while it runs, what it prints next. Returns what
 * stops it: nothing is read once the view is closed.
 */
function follow(
    element: HTMLElement,
    task: BackgroundTask,
    look: { config: TerminalConfig | null; scheme: "dark" | "light" },
    onNote: (note: OutputNote) => void,
): () => void {
    const { terminal, fit } = readOnly(element, look.config, look.scheme);
    let end: number | null = null;
    let open = true;
    const shown = (output: Awaited<ReturnType<typeof api.sessionBackgroundOutput>>) => {
        const step = outputStep(end, output);
        if (!open) {
            return;
        }
        if (step.reset) {
            terminal.reset();
        }
        terminal.write(step.text);
        end = step.end;
        if (step.note !== null) {
            onNote(step.note);
        }
    };
    // One read at a time: two that left from the same place would each bring the same piece,
    // and the second would count as not going on from the first and clear what is shown.
    let reading = false;
    const read = () => {
        if (reading) {
            return;
        }
        reading = true;
        api.sessionBackgroundOutput(task.session_id, task.task_id, end)
            .then(shown, (error) => {
                if (open) {
                    onNote(String(error));
                }
            })
            .finally(() => {
                reading = false;
            });
    };
    read();
    const following = task.state === "running" ? setInterval(read, FOLLOW_MS) : undefined;
    const resized = new ResizeObserver(() => fit.fit());
    resized.observe(element);
    return () => {
        open = false;
        clearInterval(following);
        resized.disconnect();
        terminal.dispose();
    };
}

/**
 * What a command an agent left running printed, read-only, drawn as the session's terminal is,
 * with its colours. While the command runs and this is shown, what it prints next is added;
 * nothing is read once it is closed.
 */
export function BackgroundOutput({ task }: { task: BackgroundTask }) {
    const holder = useRef<HTMLDivElement>(null);
    const settings = useStore((state) => state.settings);
    const config = useStore((state) => state.terminalConfig);
    const scheme = schemeOf(settings?.theme, useSystemDark());
    const [note, setNote] = useState<OutputNote | null>(null);

    // biome-ignore lint/correctness/useExhaustiveDependencies: followed again only when what is read or how it looks changes
    useEffect(() => {
        const element = holder.current;
        return element ? follow(element, task, { config, scheme }, setNote) : undefined;
    }, [task.session_id, task.task_id, task.state, config, scheme]);

    return (
        <div className="background-output">
            {note !== null && <p className="changes-note">{outputNote(note)}</p>}
            <div className="background-output-terminal" ref={holder} />
        </div>
    );
}

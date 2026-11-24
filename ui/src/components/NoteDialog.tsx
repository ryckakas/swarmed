import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { NoteTarget } from "../api/types";
import { useStore } from "../store";
import { Dialog } from "./Dialog";

/** The note the user keeps on a repository or on a task, opened from the sidebar. */
export function NoteDialog() {
    const noting = useStore((state) => state.noting);
    if (noting === null) {
        return null;
    }
    // A key per target, so one note's text never shows under another's name.
    return <NoteEditor key={`${noting.project}/${noting.task ?? ""}`} target={noting} />;
}

function NoteEditor({ target }: { target: NoteTarget }) {
    const askForNote = useStore((state) => state.askForNote);
    const refresh = useStore((state) => state.refresh);
    // `null` until the note has been read: nothing can be typed over what is still loading.
    const [text, setText] = useState<string | null>(null);
    const [problem, setProblem] = useState<string | null>(null);

    const box = useRef<HTMLTextAreaElement>(null);
    const loaded = text !== null;

    useEffect(() => {
        api.note(target.project, target.task).then(setText, (error) => setProblem(String(error)));
    }, [target]);
    // The note cannot be typed into while it is read, so the keyboard goes there once it can.
    useEffect(() => {
        if (loaded) {
            box.current?.focus();
        }
    }, [loaded]);

    const save = async () => {
        if (text === null) {
            return;
        }
        try {
            await api.setNote(target.project, target.task, text);
            askForNote(null);
            // The sidebar marks what has a note.
            await refresh(api);
        } catch (error) {
            setProblem(String(error));
        }
    };
    const onKeyDown = (event: KeyboardEvent) => {
        if (event.key === "Enter" && event.metaKey) {
            event.preventDefault();
            void save();
        }
    };

    return (
        <Dialog
            title={`Note on ${target.task ?? target.project}`}
            size="medium"
            onClose={() => askForNote(null)}
            dismiss="Cancel"
            problem={problem}
            actions={
                <button
                    type="button"
                    className="button button-primary"
                    disabled={text === null}
                    title="⌘ Enter"
                    onClick={() => void save()}
                >
                    Save note
                </button>
            }
        >
            <p>
                {target.task ? "For this task only." : "For the whole repository."} Kept as a text
                file on this Mac that only you can read. Agents are not shown it.
            </p>
            <textarea
                ref={box}
                className="note-text"
                aria-label="Note"
                rows={12}
                spellCheck={false}
                disabled={text === null}
                value={text ?? ""}
                onChange={(event) => setText(event.target.value)}
                onKeyDown={onKeyDown}
            />
        </Dialog>
    );
}

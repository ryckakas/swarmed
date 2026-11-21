import { useEffect, useRef } from "react";
import type { CloudTask } from "../api/types";
import { cloudProvider, cloudTitle } from "../cloud";
import { useWords } from "../useWords";

interface Props {
    task: CloudTask;
    onForget(): void;
    onKeep(): void;
}

/**
 * Asks before a cloud task is taken off the list. Nothing is stopped or deleted: the task goes
 * on at its provider and can be found there, and Swarmed only stops listing it.
 */
export function ForgetCloudDialog({ task, onForget, onKeep }: Props) {
    const words = useWords();
    const dialog = useRef<HTMLDialogElement>(null);
    // A modal <dialog> keeps the keyboard inside it and closes on Esc by itself.
    useEffect(() => {
        dialog.current?.showModal();
    }, []);
    return (
        <dialog
            ref={dialog}
            className="confirm"
            aria-labelledby="forget-cloud-title"
            onClose={onKeep}
        >
            <h2 id="forget-cloud-title">Take this cloud task off the list?</h2>
            <p className="confirm-what">{cloudTitle(task)}</p>
            <p>
                It is not stopped or deleted: it goes on at {cloudProvider(task.provider).name},
                where it can still be found. Swarmed only stops listing it here.
            </p>
            {/* Opening the dialog focuses its first button, so a stray Enter keeps the task. */}
            <div className="confirm-actions">
                <button type="button" className="button" onClick={onKeep}>
                    Keep it
                </button>
                <button type="button" className="button button-danger" onClick={onForget}>
                    {words.forgetCloud}
                </button>
            </div>
        </dialog>
    );
}

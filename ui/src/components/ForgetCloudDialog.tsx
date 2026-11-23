import type { CloudTask } from "../api/types";
import { cloudProvider, cloudTitle } from "../cloud";
import { useWords } from "../useWords";
import { Dialog } from "./Dialog";

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
    // No main button, so the keyboard starts on keeping the task and a stray Enter keeps it.
    return (
        <Dialog
            title="Take this cloud task off the list?"
            size="small"
            closeOnBackdrop={true}
            onClose={onKeep}
            dismiss="Keep it"
            actions={
                <button type="button" className="button button-danger" onClick={onForget}>
                    {words.forgetCloud}
                </button>
            }
        >
            <p className="dialog-subject">{cloudTitle(task)}</p>
            <p>
                It is not stopped or deleted: it goes on at {cloudProvider(task.provider).name},
                where it can still be found. Swarmed only stops listing it here.
            </p>
        </Dialog>
    );
}

import { useState } from "react";
import { api } from "../api";
import type { Project, Session, Task } from "../api/types";
import { repositoriesOf } from "../chats";
import { useStore } from "../store";
import { uniqueTaskName } from "../tree";
import { useWords } from "../useWords";
import { capitalized, withArticle } from "../wording";
import { Dialog } from "./Dialog";
import { IconButton } from "./IconButton";
import { Picker } from "./Picker";

interface Props {
    session: Session;
    /** The chat the session is in. */
    chat: Task;
}

/**
 * The button on a chat's session that carries it on in a repository. Not there while there is
 * no repository to carry it on in.
 */
export function CarryOnButton({ session, chat }: Props) {
    const words = useWords();
    const repositories = useStore((state) => repositoriesOf(state.projects).length);
    const [asking, setAsking] = useState(false);
    if (repositories === 0) {
        return null;
    }
    return (
        <>
            <IconButton
                label={`${words.carryOn}: a new ${words.terms.task} there that starts from all of this conversation`}
                onClick={() => setAsking(true)}
            >
                <CarryOnIcon />
            </IconButton>
            {asking && (
                <CarryOnDialog session={session} chat={chat} onClose={() => setAsking(false)} />
            )}
        </>
    );
}

/** An arrow into a box: the conversation going into a repository. */
function CarryOnIcon() {
    return <path d="M2 8h7.5M6.5 5l3 3-3 3M10.5 2.5h3v11h-3" />;
}

/**
 * Carries a chat on in a repository: a new task there, on a new branch, with a session that
 * starts from all that was said in the chat. The chat stays as it was.
 */
function CarryOnDialog({ session, chat, onClose }: Props & { onClose(): void }) {
    const words = useWords();
    const projects = useStore((state) => state.projects);
    const repositories = repositoriesOf(projects);
    const [picked, setPicked] = useState(repositories[0]?.name ?? "");
    const [busy, setBusy] = useState(false);
    const [problem, setProblem] = useState<string | null>(null);
    const target = repositories.find((one) => one.name === picked);
    const start = async () => {
        if (!target) {
            return;
        }
        setBusy(true);
        setProblem(null);
        try {
            const id = await carryOn(session.id, chat, target);
            const { refresh, open } = useStore.getState();
            await refresh(api);
            await open(api, id);
            onClose();
        } catch (error) {
            setProblem(String(error).replace(/^Error: /, ""));
            // A task whose setup failed still exists: it has to show, or trying again would
            // make a second one unseen beside it.
            await useStore.getState().refresh(api);
            setBusy(false);
        }
    };
    const { task, repository, branch } = words.terms;
    return (
        <Dialog
            title={`${words.carryOn}?`}
            size="small"
            closeOnBackdrop={!busy}
            onClose={onClose}
            problem={problem}
            busy={busy}
            aside={
                <Picker
                    label={capitalized(repository)}
                    value={picked}
                    onChange={setPicked}
                    options={repositories.map((one): [string, string] => [one.name, one.name])}
                />
            }
            actions={
                <button
                    type="button"
                    className="button button-primary"
                    disabled={!target}
                    onClick={() => void start()}
                >
                    {busy ? "Starting…" : `Start ${withArticle(task)}`}
                </button>
            }
        >
            <p>
                A new {task} in the {repository} you pick, on a new {branch}, with{" "}
                {withArticle(words.session)} that starts from all that was said here. This{" "}
                {words.chat} stays as it is.
            </p>
        </Dialog>
    );
}

/**
 * Makes a task in `repository`, named after the chat, and a session in it that starts from all
 * that was said in session `id`. Resolves to that session's id.
 */
async function carryOn(id: number, chat: Task, repository: Project): Promise<number> {
    const taken = repository.tasks.map((task) => task.name);
    const name = uniqueTaskName(chat.name, taken);
    const task = await api.taskNew(repository.name, name, null, "default");
    const fork = await api.sessionForkInto(id, repository.name, task.name);
    return fork.id;
}

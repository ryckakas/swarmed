import { useEffect } from "react";
import { api } from "./api";
import type { Pull, Task } from "./api/types";
import { useStore } from "./store";

/**
 * A task's pull request as the core last read it. The core follows it and says when it
 * changes; nothing here asks GitHub on a timer. Everything that shows the same task shares it.
 */
export function usePull({ id }: Pick<Task, "id">): Pull | null {
    return useStore((state) => state.pulls[id] ?? null);
}

/** Asks for a task's pull request once, when what shows it opens: the task panel. */
export function useReadPullOnce(project: string, { id, name }: Pick<Task, "id" | "name">) {
    const loadPull = useStore((state) => state.loadPull);
    useEffect(() => {
        void loadPull(api, project, name, id);
    }, [project, id, name, loadPull]);
}

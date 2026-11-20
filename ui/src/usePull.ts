import { useEffect } from "react";
import { api } from "./api";
import type { Pull, Task } from "./api/types";
import { pullPollMs } from "./pulls";
import { useStore } from "./store";

/**
 * A task's pull request, asked for now and again from time to time: sooner while its checks
 * run. Everything that shows the same task shares the one answer.
 */
export function usePull(project: string, { id, name }: Task): Pull | null {
    const pull = useStore((state) => state.pulls[id]);
    const loadPull = useStore((state) => state.loadPull);
    const wait = pullPollMs(pull);
    useEffect(() => {
        void loadPull(api, project, name, id);
        const timer = setInterval(() => void loadPull(api, project, name, id), wait);
        return () => clearInterval(timer);
    }, [project, id, name, wait, loadPull]);
    return pull ?? null;
}

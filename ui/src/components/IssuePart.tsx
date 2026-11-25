import { useEffect, useState } from "react";
import { api } from "../api";
import type { Issue, Task } from "../api/types";
import { problemOf } from "../branchPrefix";
import { issueSaid } from "../issues";
import { useStore } from "../store";

/** The issue a task is for as read now, or why it could not be; both `null` while it is read. */
interface IssueNow {
    issue: Issue | null;
    problem: string | null;
}

/**
 * The issue a task was started for, in the task panel: its number, whether it is still open,
 * and its title, read from GitHub when the panel opens, with ways to read it whole. Nothing for
 * a task that is for none.
 */
export function IssuePart({ project, task }: { project: string; task: Task }) {
    const number = task.issue;
    const now = useTaskIssue(project, task.name, number);
    const showIssues = useStore((state) => state.showIssues);
    const report = useStore((state) => state.report);
    if (number === null) {
        return null;
    }
    const said = issueSaid(number, now.issue);
    return (
        <div className="panel-issue" data-state={now.issue?.state}>
            <p className="panel-issue-said" title={said}>
                {said}
                {now.problem !== null && (
                    <span className="panel-issue-problem"> (not read: {now.problem})</span>
                )}
            </p>
            <div className="panel-actions">
                <button
                    type="button"
                    className="button"
                    onClick={() => showIssues({ project, issue: number })}
                >
                    Read it
                </button>
                {now.issue && (
                    <button
                        type="button"
                        className="button"
                        title={`Opens ${now.issue.url}`}
                        onClick={() => now.issue && api.openLink(now.issue.url).catch(report)}
                    >
                        Open on GitHub
                    </button>
                )}
            </div>
        </div>
    );
}

/** The task's issue, read once each time the panel shows the task. */
function useTaskIssue(project: string, name: string, number: number | null): IssueNow {
    const [now, setNow] = useState<IssueNow>({ issue: null, problem: null });
    useEffect(() => {
        setNow({ issue: null, problem: null });
        if (number === null) {
            return;
        }
        let current = true;
        api.taskIssue(project, name)
            .then((issue) => current && setNow({ issue, problem: null }))
            .catch(
                (error: unknown) => current && setNow({ issue: null, problem: problemOf(error) }),
            );
        return () => {
            current = false;
        };
    }, [project, name, number]);
    return now;
}

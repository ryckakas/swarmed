import { useEffect, useState } from "react";
import { actionLabel, actionTooltip, splitActions } from "../actionbar";
import { api } from "../api";
import type { Action } from "../api/types";
import { useStore } from "../store";
import { useWords } from "../useWords";
import { Picker } from "./Picker";

/**
 * The user's own buttons for the session on screen: those kept for every repository and those
 * of this one. However many there are, the first few are buttons, on as many lines as they
 * need, and the rest are in a menu. Nothing at all while the user has made none.
 */
export function ActionBar({ project }: { project: string }) {
    const words = useWords();
    const sessionId = useStore((state) => state.selectedSessionId);
    const changed = useStore((state) => state.actionsChanged);
    const [actions, setActions] = useState<Action[]>([]);

    // biome-ignore lint/correctness/useExhaustiveDependencies: read again when the actions were changed
    useEffect(() => {
        let current = true;
        api.actions(project)
            .then((offered) => current && setActions(offered))
            .catch(useStore.getState().report);
        return () => {
            current = false;
        };
    }, [project, changed]);

    if (actions.length === 0) {
        return null;
    }
    const run = (action: Action) => {
        if (sessionId === null) {
            return;
        }
        const { report, runInTerminal } = useStore.getState();
        if (action.command === "") {
            api.sessionSend(sessionId, action.prompt).catch(report);
        } else {
            runInTerminal(sessionId, action.command);
        }
    };
    const [shown, rest] = splitActions(actions);
    return (
        <fieldset className="panel-actions action-bar" aria-label={words.actions}>
            {shown.map((action) => (
                <button
                    key={`${action.of_project}-${action.name}`}
                    type="button"
                    className="button action-button"
                    title={actionTooltip(action)}
                    disabled={sessionId === null}
                    onClick={() => run(action)}
                >
                    {action.icon !== "" && <span aria-hidden="true">{action.icon}</span>}
                    {action.name}
                </button>
            ))}
            {rest.length > 0 && (
                <Picker
                    label={words.moreActions}
                    value=""
                    onChange={(name) => {
                        const chosen = rest.find((action) => action.name === name);
                        if (chosen) {
                            run(chosen);
                        }
                    }}
                    options={[
                        ["", words.moreActions],
                        ...rest.map((action): [string, string] => [
                            action.name,
                            actionLabel(action),
                        ]),
                    ]}
                />
            )}
        </fieldset>
    );
}

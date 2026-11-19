import { type FormEvent, useEffect, useState } from "react";
import { api } from "../api";
import type { AgentSetup } from "../api/types";
import { useStore } from "../store";

/**
 * The agents sessions can run on: whether each is installed, a button that installs it, how to
 * sign in to it, and the models it offers. What used to be commands in a terminal and a file
 * edited by hand.
 */
export function AgentsSettings() {
    const report = useStore((state) => state.report);
    const [setups, setSetups] = useState<AgentSetup[]>([]);
    // biome-ignore lint/correctness/useExhaustiveDependencies: read once, when the settings open
    useEffect(() => {
        api.agentSetups().then(setSetups).catch(report);
    }, []);
    const replace = (changed: AgentSetup) => {
        setSetups((all) => all.map((setup) => (setup.id === changed.id ? changed : setup)));
        // The agent and model menus elsewhere read the agents again.
        useStore.getState().agentsChanged();
    };

    return (
        <section className="settings-group" aria-label="Agents">
            <h3>Agents</h3>
            <p className="settings-hint">
                Sessions run on Claude Code unless you choose another agent. Swarmed installs an
                agent when you ask it to; signing in stays between you and the agent.
            </p>
            {setups.map((setup) => (
                <AgentRow key={setup.id} setup={setup} onChange={replace} />
            ))}
        </section>
    );
}

/** What is to be said about an agent's state, and what the user can do next. */
function stateOf(setup: AgentSetup): string {
    if (setup.installed === null) {
        return "Not installed";
    }
    return setup.configured ? `Ready, at ${setup.installed}` : `Installed at ${setup.installed}`;
}

function AgentRow({ setup, onChange }: { setup: AgentSetup; onChange(setup: AgentSetup): void }) {
    const report = useStore((state) => state.report);
    // Asked before anything is installed: the command is shown and has to be agreed to.
    const [asking, setAsking] = useState(false);
    const [installing, setInstalling] = useState(false);
    const printed = useStore((state) => state.hookLine);

    const install = () => {
        setAsking(false);
        setInstalling(true);
        useStore.setState({ hookLine: null });
        api.agentInstall(setup.id)
            .then(onChange)
            .catch(report)
            .finally(() => setInstalling(false));
    };
    const configure = (models: string[] | null) =>
        api.agentConfigure(setup.id, models).then(onChange).catch(report);

    return (
        <div className="agent-row">
            <div className="settings-row">
                <div>
                    <p className="settings-label">{setup.name}</p>
                    <p className="settings-hint">{setup.about}</p>
                    <p className="settings-hint">{stateOf(setup)}</p>
                    {setup.installed !== null && <p className="settings-hint">{setup.sign_in}</p>}
                </div>
                {setup.installed === null && !asking && (
                    <button
                        type="button"
                        className="button"
                        disabled={installing}
                        onClick={() => setAsking(true)}
                    >
                        {installing ? "Installing…" : "Install"}
                    </button>
                )}
                {setup.installed !== null && !setup.configured && (
                    <button type="button" className="button" onClick={() => void configure(null)}>
                        Set up
                    </button>
                )}
            </div>
            {asking && (
                <div className="agent-ask">
                    <p>
                        This runs <code>{setup.install_command}</code> on this Mac.
                    </p>
                    <button type="button" className="button" onClick={() => setAsking(false)}>
                        Not now
                    </button>
                    <button type="button" className="button button-primary" onClick={install}>
                        Install
                    </button>
                </div>
            )}
            {installing && <p className="agent-output mono">{printed ?? "Starting…"}</p>}
            {setup.configured && !setup.built_in && (
                <Models models={setup.models} onChange={(models) => void configure(models)} />
            )}
        </div>
    );
}

/** The models an agent's menu offers: taken out one by one, added from OpenRouter's own list. */
function Models({ models, onChange }: { models: string[]; onChange(models: string[]): void }) {
    const [typed, setTyped] = useState("");
    // Every model OpenRouter has, fetched when the box is first used; typing narrows them down.
    const [known, setKnown] = useState<string[] | null>(null);
    const fetchKnown = () => {
        if (known === null) {
            setKnown([]);
            // Without the list the box still takes any id typed into it.
            api.openrouterModels()
                .then(setKnown)
                .catch(() => undefined);
        }
    };
    const add = (event: FormEvent) => {
        event.preventDefault();
        const model = typed.trim();
        if (model !== "" && !models.includes(model)) {
            onChange([...models, model]);
        }
        setTyped("");
    };
    return (
        <div className="agent-models">
            <p className="settings-hint">Models offered when a session runs on it:</p>
            <ul className="model-chips">
                {models.map((model) => (
                    <li key={model} className="model-chip mono">
                        {model}
                        <button
                            type="button"
                            aria-label={`Stop offering ${model}`}
                            title="Stop offering this model"
                            onClick={() => onChange(models.filter((other) => other !== model))}
                        >
                            ×
                        </button>
                    </li>
                ))}
            </ul>
            <form className="key-row" onSubmit={add}>
                <input
                    className="key-input"
                    aria-label="A model to offer"
                    placeholder="Add a model, such as openrouter/openai/…"
                    list="openrouter-models"
                    spellCheck={false}
                    value={typed}
                    onFocus={fetchKnown}
                    onChange={(event) => setTyped(event.target.value)}
                />
                <datalist id="openrouter-models">
                    {(known ?? []).map((model) => (
                        <option key={model} value={model} />
                    ))}
                </datalist>
                <button type="submit" className="button" disabled={typed.trim() === ""}>
                    Add
                </button>
            </form>
        </div>
    );
}

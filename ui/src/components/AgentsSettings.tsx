import { type FormEvent, useEffect, useState } from "react";
import { api } from "../api";
import type { AgentSetup } from "../api/types";
import { iconForAgent } from "../providerIcons";
import { useStore } from "../store";
import { ProviderMark } from "./ProviderMark";

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

/** Whether sessions can run on the agent as things are: it is installed and set up. */
export function isReady(setup: AgentSetup): boolean {
    return setup.installed !== null && setup.configured;
}

/** The green tick in front of an agent that is ready to use. */
function ReadyMark() {
    return (
        <svg className="agent-ready" viewBox="0 0 16 16" role="img" aria-label="Ready to use">
            <rect x="1.5" y="1.5" width="13" height="13" rx="3.5" />
            <path d="M4.6 8.3l2.3 2.3 4.5-5" />
        </svg>
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
                    <p className="settings-label agent-name">
                        {isReady(setup) && <ReadyMark />}
                        <ProviderMark icon={iconForAgent(setup.id)} />
                        {setup.name}
                    </p>
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
            {setup.configured && setup.model_source === "agent" && (
                <OwnModels setup={setup} onChange={onChange} />
            )}
            {setup.configured && setup.model_source === "openrouter" && (
                <Models models={setup.models} onChange={(models) => void configure(models)} />
            )}
        </div>
    );
}

/**
 * The models of an agent that knows them itself: read from the agent, not typed in. Asked
 * for by itself the first time, and again whenever the user wants the list brought up to date.
 */
function OwnModels({ setup, onChange }: { setup: AgentSetup; onChange(setup: AgentSetup): void }) {
    const report = useStore((state) => state.report);
    const [asking, setAsking] = useState(false);
    const ask = () => {
        setAsking(true);
        api.agentModelsRefresh(setup.id)
            .then(onChange)
            .catch(report)
            .finally(() => setAsking(false));
    };
    const unknown = setup.models.length === 0;
    // biome-ignore lint/correctness/useExhaustiveDependencies: asked once, when nothing is known yet
    useEffect(() => {
        if (unknown) {
            ask();
        }
    }, []);
    return (
        <div className="agent-models">
            <p className="settings-hint">{ownModelsLine(setup, asking)}</p>
            <ul className="model-chips">
                {setup.models.map((model) => (
                    <li key={model} className="model-chip mono">
                        {model}
                    </li>
                ))}
            </ul>
            <button type="button" className="button" disabled={asking} onClick={ask}>
                {asking ? "Asking…" : "Read the models again"}
            </button>
        </div>
    );
}

/** What stands above the models an agent told Swarmed about. */
export function ownModelsLine(setup: AgentSetup, asking: boolean): string {
    if (setup.models.length > 0) {
        return `Models ${setup.name} offers on your account. Its own default is used unless you pick one:`;
    }
    return asking
        ? `Asking ${setup.name} which models it offers…`
        : `${setup.name} has not said which models it offers. Its own default is used.`;
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

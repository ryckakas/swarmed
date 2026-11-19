import { type FormEvent, useEffect, useState } from "react";
import { api } from "../api";
import type { McpServerInput, McpServerView } from "../api/types";
import { useStore } from "../store";

/** A server as the form holds it while it is being written. */
interface Draft {
    /** The name of the server being changed; `null` for a new one. */
    editing: string | null;
    name: string;
    kind: "stdio" | "http";
    target: string;
    /** Each secret's name, the value typed for it, and whether one is stored already. */
    secrets: { name: string; value: string; stored: boolean }[];
    enabled: boolean;
}

const EMPTY: Draft = {
    editing: null,
    name: "",
    kind: "stdio",
    target: "",
    secrets: [],
    enabled: true,
};

function draftOf(server: McpServerView): Draft {
    return {
        editing: server.name,
        name: server.name,
        kind: server.kind === "http" ? "http" : "stdio",
        target: server.target,
        secrets: server.secrets.map((name) => ({ name, value: "", stored: true })),
        enabled: server.enabled,
    };
}

/** What is saved for a draft. A secret left empty keeps the value it has. */
function inputOf(draft: Draft): McpServerInput {
    return {
        name: draft.name.trim(),
        command: draft.kind === "stdio" ? draft.target : "",
        url: draft.kind === "http" ? draft.target : "",
        secrets: draft.secrets
            .filter((secret) => secret.name.trim() !== "")
            .map((secret) => [secret.name.trim(), secret.value === "" ? null : secret.value]),
        enabled: draft.enabled,
    };
}

/**
 * MCP servers: the ones set up here, which every agent Swarmed runs is given, and Claude
 * Code's own, which are only shown. Secrets are typed in and never shown again.
 */
export function McpSettings() {
    const report = useStore((state) => state.report);
    const [servers, setServers] = useState<McpServerView[]>([]);
    const [draft, setDraft] = useState<Draft | null>(null);
    // biome-ignore lint/correctness/useExhaustiveDependencies: read once, when the tab opens
    useEffect(() => {
        api.mcpServers().then(setServers).catch(report);
    }, []);

    const own = servers.filter((server) => server.origin === "swarmed");
    const claude = servers.filter((server) => server.origin !== "swarmed");
    const save = (input: McpServerInput) =>
        api
            .mcpSave(input)
            .then(setServers)
            .then(() => setDraft(null))
            .catch(report);
    const remove = (name: string) => api.mcpRemove(name).then(setServers).catch(report);

    return (
        <section className="settings-group" aria-label="MCP servers">
            <h3>MCP servers</h3>
            <p className="settings-hint">
                Servers set up here are given to every agent Swarmed runs, from the next session
                started. Their secrets are kept in the macOS Keychain and cannot be shown again.
            </p>
            {own.map((server) => (
                <OwnRow
                    key={server.name}
                    server={server}
                    onEdit={() => setDraft(draftOf(server))}
                    onToggle={() =>
                        void save({ ...inputOf(draftOf(server)), enabled: !server.enabled })
                    }
                    onRemove={() => void remove(server.name)}
                />
            ))}
            {draft ? (
                <ServerForm
                    draft={draft}
                    onChange={setDraft}
                    onSave={() => void save(inputOf(draft))}
                    onCancel={() => setDraft(null)}
                />
            ) : (
                <button type="button" className="button" onClick={() => setDraft(EMPTY)}>
                    Add a server
                </button>
            )}
            <h3 className="settings-subhead">From Claude Code</h3>
            <p className="settings-hint">
                {claude.length === 0
                    ? "None found. Claude Code's own servers and connectors show here once a session has run."
                    : "Claude Code's own servers and the connectors of your Claude account. They are managed in Claude Code; other agents are given the ones Swarmed can read."}
            </p>
            <ul className="mcp-list">
                {claude.map((server) => (
                    <li key={server.name} className="mcp-row">
                        <span className="mcp-name">{server.name}</span>
                        <Status status={server.status} />
                        <span className="mcp-target mono" title={server.target}>
                            {server.target}
                        </span>
                    </li>
                ))}
            </ul>
        </section>
    );
}

/** What Claude Code last said of a server, coloured by whether it is usable. */
function Status({ status }: { status: string | null }) {
    if (status === null) {
        return <span className="mcp-status">not seen yet</span>;
    }
    const tone = status === "connected" || status === "passed" ? "good" : "off";
    return <span className={`mcp-status mcp-status-${tone}`}>{status}</span>;
}

interface OwnRowProps {
    server: McpServerView;
    onEdit(): void;
    onToggle(): void;
    onRemove(): void;
}

function OwnRow({ server, onEdit, onToggle, onRemove }: OwnRowProps) {
    return (
        <div className="settings-row">
            <div className="mcp-own">
                <p className="settings-label">
                    {server.name} <Status status={server.status} />
                </p>
                <p className="settings-hint mono">{server.target}</p>
                {server.secrets.length > 0 && (
                    <p className="settings-hint">Secrets: {server.secrets.join(", ")}</p>
                )}
            </div>
            <label className="settings-check">
                <input type="checkbox" checked={server.enabled} onChange={onToggle} />
                On
            </label>
            <button type="button" className="button" onClick={onEdit}>
                Edit
            </button>
            <button type="button" className="button" onClick={onRemove}>
                Remove
            </button>
        </div>
    );
}

interface FormProps {
    draft: Draft;
    onChange(draft: Draft): void;
    onSave(): void;
    onCancel(): void;
}

function ServerForm({ draft, onChange, onSave, onCancel }: FormProps) {
    const submit = (event: FormEvent) => {
        event.preventDefault();
        onSave();
    };
    const setSecret = (index: number, change: Partial<Draft["secrets"][number]>) =>
        onChange({
            ...draft,
            secrets: draft.secrets.map((secret, at) =>
                at === index ? { ...secret, ...change } : secret,
            ),
        });
    const program = draft.kind === "stdio";
    return (
        <form className="mcp-form" onSubmit={submit}>
            <div className="key-row">
                <input
                    className="key-input"
                    aria-label="Server name"
                    placeholder="Name, such as tracker"
                    spellCheck={false}
                    disabled={draft.editing !== null}
                    value={draft.name}
                    onChange={(event) => onChange({ ...draft, name: event.target.value })}
                />
                <select
                    className="picker"
                    aria-label="How the server is reached"
                    value={draft.kind}
                    onChange={(event) =>
                        onChange({
                            ...draft,
                            kind: event.target.value as Draft["kind"],
                            target: "",
                        })
                    }
                >
                    <option value="stdio">A command to start</option>
                    <option value="http">An address</option>
                </select>
            </div>
            <input
                className="key-input"
                aria-label={program ? "Command" : "Address"}
                placeholder={program ? "npx -y @acme/tracker-mcp" : "https://example.com/mcp"}
                spellCheck={false}
                value={draft.target}
                onChange={(event) => onChange({ ...draft, target: event.target.value })}
            />
            <p className="settings-hint">
                {program
                    ? "Secrets are given to the command as environment variables."
                    : "Secrets are sent as headers with every request."}
            </p>
            {draft.secrets.map((secret, index) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: rows are only added at the end, and their names change as they are typed
                <div className="key-row" key={index}>
                    <input
                        className="key-input"
                        aria-label="Secret name"
                        placeholder={program ? "TRACKER_TOKEN" : "Authorization"}
                        spellCheck={false}
                        value={secret.name}
                        onChange={(event) => setSecret(index, { name: event.target.value })}
                    />
                    <input
                        type="password"
                        className="key-input"
                        aria-label={`Value of ${secret.name || "the secret"}`}
                        placeholder={secret.stored ? "Stored. Type to replace it" : "Value"}
                        autoComplete="off"
                        value={secret.value}
                        onChange={(event) => setSecret(index, { value: event.target.value })}
                    />
                </div>
            ))}
            <div className="confirm-actions">
                <button
                    type="button"
                    className="button"
                    onClick={() =>
                        onChange({
                            ...draft,
                            secrets: [...draft.secrets, { name: "", value: "", stored: false }],
                        })
                    }
                >
                    Add a secret
                </button>
                <button type="button" className="button" onClick={onCancel}>
                    Cancel
                </button>
                <button
                    type="submit"
                    className="button button-primary"
                    disabled={draft.name.trim() === "" || draft.target.trim() === ""}
                >
                    Save server
                </button>
            </div>
        </form>
    );
}

import { type FormEvent, useEffect, useState } from "react";
import { api } from "../api";
import type { ApiKeyInfo } from "../api/types";
import { iconForKey } from "../providerIcons";
import { useStore } from "../store";
import { ProviderMark } from "./ProviderMark";

/** Keys for the providers that agents other than Claude Code talk to. Write-only by design. */
export function ApiKeys() {
    const report = useStore((state) => state.report);
    const [keys, setKeys] = useState<ApiKeyInfo[]>([]);
    const read = () => api.apiKeys().then(setKeys).catch(report);
    // biome-ignore lint/correctness/useExhaustiveDependencies: read once, when the settings open
    useEffect(() => {
        void read();
    }, []);

    return (
        <section className="settings-group" aria-label="API keys">
            <h3>API keys</h3>
            <p className="settings-hint">
                Kept in the macOS Keychain and given to agents other than Claude Code when a session
                starts. Claude Code keeps using your Claude login. A stored key cannot be shown
                again, only replaced or removed.
            </p>
            {keys.map((key) => (
                <KeyRow key={key.name} info={key} onChange={read} />
            ))}
        </section>
    );
}

function KeyRow({ info, onChange }: { info: ApiKeyInfo; onChange(): void }) {
    const report = useStore((state) => state.report);
    const [value, setValue] = useState("");
    const save = (event: FormEvent) => {
        event.preventDefault();
        api.setApiKey(info.name, value)
            .then(() => setValue(""))
            .then(onChange)
            .catch(report);
    };
    const remove = () => api.deleteApiKey(info.name).then(onChange).catch(report);
    return (
        <form className="key-row" onSubmit={save}>
            <div className="key-name">
                <p className="settings-label agent-name">
                    <ProviderMark icon={iconForKey(info.name)} />
                    {info.provider}
                </p>
                <p className="settings-hint">{info.stored ? "A key is stored" : "No key"}</p>
            </div>
            <input
                type="password"
                className="key-input"
                aria-label={`${info.provider} API key`}
                placeholder={info.stored ? "Paste a new key to replace it" : "Paste the key"}
                autoComplete="off"
                spellCheck={false}
                value={value}
                onChange={(event) => setValue(event.target.value)}
            />
            <button type="submit" className="button" disabled={value.trim() === ""}>
                Save
            </button>
            <button type="button" className="button" disabled={!info.stored} onClick={remove}>
                Remove
            </button>
        </form>
    );
}

import type { ProviderIcon } from "../providerIcons";

/**
 * A provider's mark, small enough to sit in a line of text. `label` is what it is called where
 * it stands, such as the agent's name; without one it is decoration beside a name already there.
 */
export function ProviderMark({ icon, label }: { icon: ProviderIcon; label?: string }) {
    return (
        <svg
            className="provider-mark"
            viewBox="0 0 24 24"
            role={label ? "img" : undefined}
            aria-label={label}
            aria-hidden={label ? undefined : true}
            style={icon.color ? { color: icon.color } : undefined}
        >
            {label && <title>{label}</title>}
            <path d={icon.path} />
        </svg>
    );
}

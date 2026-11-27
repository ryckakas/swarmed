import type { ComponentProps } from "react";
import { api } from "../api";
import { useStore } from "../store";

/**
 * A link in a conversation, the agent's or the user's. It opens in the user's browser: followed
 * here it would replace Swarmed's window with a web page that has no way back.
 */
export function Link({
    href,
    children,
    node: _node,
    ...rest
}: ComponentProps<"a"> & { node?: unknown }) {
    const report = useStore((state) => state.report);
    return (
        <a
            {...rest}
            href={href}
            onClick={(event) => {
                event.preventDefault();
                if (href) {
                    api.openLink(href).catch(report);
                }
            }}
        >
            {children}
        </a>
    );
}

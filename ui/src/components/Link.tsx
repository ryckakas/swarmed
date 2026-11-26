import type { ComponentProps } from "react";
import { api } from "../api";
import { hasLinks, saidLinks } from "../saidLinks";
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

/** Words the user wrote, each address in them a link that opens in their browser. */
export function WithLinks({ text }: { text: string }) {
    const pieces = saidLinks(text);
    if (!hasLinks(pieces)) {
        return <span>{text}</span>;
    }
    return (
        <span>
            {pieces.map((piece, index) =>
                typeof piece === "string" ? (
                    piece
                ) : (
                    // biome-ignore lint/suspicious/noArrayIndexKey: the pieces of a sent message never change
                    <Link key={index} className="said-link" href={piece.address}>
                        {piece.shown}
                    </Link>
                ),
            )}
        </span>
    );
}

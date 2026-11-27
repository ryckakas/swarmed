import { isValidElement, type ReactNode } from "react";

/** The text inside rendered Markdown, however deeply it is wrapped. */
export function textOf(node: ReactNode): string {
    if (typeof node === "string" || typeof node === "number") {
        return String(node);
    }
    if (Array.isArray(node)) {
        return node.map(textOf).join("");
    }
    return isValidElement<{ children?: ReactNode }>(node) ? textOf(node.props.children) : "";
}

/**
 * What the copy button of a code block puts on the clipboard: the block's code as written,
 * without the markup that colours it, and without the one newline Markdown leaves at the end
 * of a block. The fence and the language's name are never in a block's text. A block still
 * being written gives what has arrived, which may not end in a newline yet.
 */
export function copiedText(block: ReactNode): string {
    const text = textOf(block);
    return text.endsWith("\n") ? text.slice(0, -1) : text;
}

/**
 * How wide the scroll bar beside a block is, from its box's width (`offsetWidth`), the width
 * that shows its text (`clientWidth`) and its borders: what the copy button stands clear of. 0
 * where the block does not scroll, and where scroll bars take no room and float over it.
 */
export function barBeside(box: number, shown: number, borders: number): number {
    return Math.max(0, box - shown - borders);
}

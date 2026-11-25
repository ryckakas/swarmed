// The user's comments on a task's diff, gathered before they are sent to an agent as one
// message. Pure, so it can be tested.

/** One comment on one line of a diff. */
export interface ReviewComment {
    path: string;
    /** The line's number: in the new file, or in the old one for a line that was removed. */
    line: number;
    /** Whether `line` counts in the file as it is now, or as it was. */
    side: "new" | "old";
    /** The line itself, so the agent finds it even after the numbers moved. */
    code: string;
    /** What the user has to say about it. */
    text: string;
}

/**
 * `comment`, made at the place of a line that reads `code`, if it was made on that line. One
 * turn's diff of a file and the whole task's count their lines alike and hold other code
 * there: a comment shows only where the line it was made on stands.
 */
export function madeOn(comment: ReviewComment | undefined, code: string) {
    return comment?.code === code ? comment : undefined;
}

/** What tells one commented line from another. */
export function commentKey(comment: Pick<ReviewComment, "path" | "line" | "side">): string {
    return `${comment.path}:${comment.side}:${comment.line}`;
}

/**
 * The comments with `comment` put in: in place of the one on the same line if there is one,
 * and with nothing in its place if its text is empty.
 */
export function withComment(comments: ReviewComment[], comment: ReviewComment): ReviewComment[] {
    const key = commentKey(comment);
    const others = comments.filter((other) => commentKey(other) !== key);
    return comment.text.trim() === "" ? others : [...others, comment];
}

/** The comments in the order to read them: by file, then by line. */
function ordered(comments: ReviewComment[]): ReviewComment[] {
    return [...comments].sort((a, b) => a.path.localeCompare(b.path) || a.line - b.line);
}

/** The message that hands the comments to an agent. Nothing if there are none. */
export function reviewMessage(comments: ReviewComment[]): string {
    if (comments.length === 0) {
        return "";
    }
    const blocks = ordered(comments).map((comment) => {
        const where = `${comment.path}:${comment.line}${comment.side === "old" ? " (a line you removed)" : ""}`;
        return `${where}\n> ${comment.code.trim()}\n${comment.text.trim()}`;
    });
    return [
        "I reviewed the changes in this worktree. My comments, each under the line it is about:",
        ...blocks,
        "Address each one, then tell me what you changed.",
    ].join("\n\n");
}

import { useMemo } from "react";
import { api } from "../api";
import type { Project } from "../api/types";
import { opensInEditor } from "../files";
import { drawnApart, type SaidPiece, saidPieces } from "../said";
import { fileAddress, inWorktree, type SaidPath } from "../saidPaths";
import { useStore } from "../store";
import { findSession, findSessionHome } from "../tree";
import { Link } from "./Link";

/**
 * Words the user wrote to a session: each address in them a link that opens in their browser,
 * and code and paths drawn as a reply's inline code is, a path in the session's worktree
 * opening in their editor.
 */
export function SaidText({ text, sessionId }: { text: string; sessionId: number }) {
    const pieces = useMemo(() => saidPieces(text), [text]);
    if (!drawnApart(pieces)) {
        return <span>{text}</span>;
    }
    return (
        <span>
            {pieces.map((piece, index) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: the pieces of a sent message never change
                <Piece key={index} piece={piece} sessionId={sessionId} />
            ))}
        </span>
    );
}

function Piece({ piece, sessionId }: { piece: SaidPiece; sessionId: number }) {
    if (typeof piece === "string") {
        return piece;
    }
    if ("address" in piece) {
        return (
            <Link className="said-link" href={piece.address}>
                {piece.shown}
            </Link>
        );
    }
    if ("code" in piece) {
        return <code className="said-code">{piece.code}</code>;
    }
    return <PathPiece piece={piece} sessionId={sessionId} />;
}

/**
 * A path the user wrote. One in the worktree of the session's task, or a chat's folder, opens
 * in their editor when pressed, at its top: the line after it is only shown. The core refuses
 * one that is not there or leads out through a link, which is said as the Files tab says it.
 * It is a link to the file rather than a button, so that a long one wraps with the words: the
 * press is taken over, and the window would refuse to follow it anyway.
 */
function PathPiece({ piece, sessionId }: { piece: SaidPath; sessionId: number }) {
    const report = useStore((state) => state.report);
    const project = useStore((state) => homeOf(state.projects, sessionId)?.project.name);
    const task = useStore((state) => homeOf(state.projects, sessionId)?.task);
    const file = task ? inWorktree(piece.path, task.worktree_path) : null;
    if (project === undefined || task === undefined || file === null) {
        return <code className="said-code">{piece.shown}</code>;
    }
    return (
        <a
            className="said-code said-path"
            href={fileAddress(task.worktree_path, file)}
            title={opensInEditor(file)}
            onClick={(event) => {
                event.preventDefault();
                api.taskOpenFile(project, task.name, file).catch(report);
            }}
        >
            {piece.shown}
        </a>
    );
}

/** The task of the session `sessionId`, with its project. */
function homeOf(projects: Project[], sessionId: number) {
    const session = findSession(projects, sessionId);
    return session && findSessionHome(projects, session);
}

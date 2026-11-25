import { useEffect, useState } from "react";
import { api } from "../api";
import { keptThumbnail } from "../keptImages";
import { useStore } from "../store";
import { PREVIEW_PX, shrink, THUMBNAIL_PX } from "../thumbnails";
import { fileName } from "../transcript";
import { Dialog } from "./Dialog";

interface Props {
    sessionId: number;
    /** The images, as the core listed them on the call's result: never a path built here. */
    paths: readonly string[];
}

/**
 * The images a tool call, or a run of folded calls, returned: small, in a row under it. Each
 * shows large on a press.
 */
export function ToolImages({ sessionId, paths }: Props) {
    const [shown, setShown] = useState<string | null>(null);
    if (paths.length === 0) {
        return null;
    }
    return (
        <>
            <ul className="tool-images" aria-label="Images it returned">
                {paths.map((path) => (
                    <li key={path}>
                        <Thumbnail
                            sessionId={sessionId}
                            path={path}
                            most={THUMBNAIL_PX}
                            onPress={() => setShown(path)}
                        />
                    </li>
                ))}
            </ul>
            {shown !== null && (
                <ImageDialog sessionId={sessionId} path={shown} onClose={() => setShown(null)} />
            )}
        </>
    );
}

/** The last image the agent's tools returned in a turn, larger, at the end of its reply. */
export function LastImage({ sessionId, path }: { sessionId: number; path: string }) {
    const [shown, setShown] = useState(false);
    return (
        <>
            <Thumbnail
                sessionId={sessionId}
                path={path}
                most={PREVIEW_PX}
                large={true}
                onPress={() => setShown(true)}
            />
            {shown && (
                <ImageDialog sessionId={sessionId} path={path} onClose={() => setShown(false)} />
            )}
        </>
    );
}

/** A smaller copy of a kept image, `null` until it is read, or `false` if it cannot be. */
function useThumbnail(sessionId: number, path: string, most: number): string | null | false {
    const [source, setSource] = useState<string | null | false>(null);
    useEffect(() => {
        let current = true;
        keptThumbnail(sessionId, path, most, api.sessionImage, shrink).then(
            (found) => current && setSource(found),
            () => current && setSource(false),
        );
        return () => {
            current = false;
        };
    }, [sessionId, path, most]);
    return source;
}

interface ThumbnailProps {
    sessionId: number;
    path: string;
    most: number;
    large?: boolean;
    onPress(): void;
}

/**
 * One image, drawn small. It takes its room at once, so that the transcript does not move once
 * it is read; one that is gone, deleted or kept on another machine, says so in its place.
 */
function Thumbnail({ sessionId, path, most, large = false, onPress }: ThumbnailProps) {
    const source = useThumbnail(sessionId, path, most);
    const name = fileName(path);
    const kind = large ? "tool-image tool-image-large" : "tool-image";
    if (source === false) {
        return (
            <span className={`${kind} tool-image-gone`} title={path}>
                image not found
            </span>
        );
    }
    return (
        <button
            type="button"
            className={kind}
            title={`${name}: show it large`}
            aria-label={`Show ${name} large`}
            onClick={onPress}
        >
            {source !== null && <img src={source} alt="" />}
        </button>
    );
}

/**
 * A kept image shown large in the window, read whole for as long as it is open, with Open to
 * hand the file to the system's viewer.
 */
function ImageDialog(props: { sessionId: number; path: string; onClose(): void }) {
    const { sessionId, path, onClose } = props;
    const [source, setSource] = useState<string | null>(null);
    const [problem, setProblem] = useState<string | null>(null);
    useEffect(() => {
        let current = true;
        api.sessionImage(sessionId, path).then(
            (found) => current && setSource(found),
            (error: unknown) => current && setProblem(String(error)),
        );
        return () => {
            current = false;
        };
    }, [sessionId, path]);
    const open = () => {
        api.sessionImageOpen(sessionId, path).then(onClose, useStore.getState().report);
    };
    return (
        <Dialog
            title={fileName(path)}
            size="wide"
            closeOnBackdrop={true}
            onClose={onClose}
            problem={problem}
            actions={
                <button
                    type="button"
                    className="button button-primary"
                    title="Open it in the system's viewer"
                    onClick={open}
                >
                    Open
                </button>
            }
        >
            <div className="image-large">{source !== null && <img src={source} alt={path} />}</div>
        </Dialog>
    );
}

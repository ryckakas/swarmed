import { useEffect, useState } from "react";
import { api } from "../api";
import { imageSource, isPastedImage, type PastedImage } from "../images";
import { useStore } from "../store";
import { fileName } from "../transcript";
import { useImageFolder } from "../usePastedImages";

interface Props {
    files: string[];
    /** Given where a file can be taken off again, as in a message still being written. */
    onRemove?: (file: string) => void;
    /** The session the files go to: those pasted into it show as the images they are. */
    sessionId?: number | undefined;
    /** Images pasted into a message still being written, not kept anywhere yet. */
    images?: PastedImage[];
    onRemoveImage?: (image: PastedImage) => void;
}

/**
 * Files the user points the agent at: each by its name, with its whole path on hover, except
 * images pasted into the session, which show as small images. Pressing a kept one opens it in
 * the system's viewer.
 */
export function FileChips({ files, onRemove, sessionId, images = [], onRemoveImage }: Props) {
    const folder = useImageFolder(sessionId);
    if (files.length === 0 && images.length === 0) {
        return null;
    }
    return (
        <ul className="file-chips" aria-label="Attached files">
            {files.map((file) =>
                sessionId !== undefined && isPastedImage(file, folder) ? (
                    <KeptImage
                        key={file}
                        sessionId={sessionId}
                        file={file}
                        onRemove={onRemove && (() => onRemove(file))}
                    />
                ) : (
                    <FileChip
                        key={file}
                        file={file}
                        onRemove={onRemove && (() => onRemove(file))}
                    />
                ),
            )}
            {images.map((image) => (
                <li key={`pasted-${image.key}`} className="image-chip" title="Pasted image">
                    <img src={imageSource(image)} alt="Pasted" />
                    {onRemoveImage && (
                        <Remove name="pasted image" onRemove={() => onRemoveImage(image)} />
                    )}
                </li>
            ))}
        </ul>
    );
}

/** The × that takes a file off a message still being written. */
function Remove({ name, onRemove }: { name: string; onRemove: () => void }) {
    return (
        <button
            type="button"
            className="chip-remove"
            aria-label={`Remove ${name}`}
            title="Remove"
            // Leaves the focus where the text is, so the box it is in stays open and typed in.
            onMouseDown={(event) => event.preventDefault()}
            onClick={onRemove}
        >
            ×
        </button>
    );
}

function FileChip({ file, onRemove }: { file: string; onRemove: (() => void) | undefined }) {
    return (
        <li className="file-chip" title={file}>
            <svg viewBox="0 0 16 16" aria-hidden="true">
                <path d="M4 1.8h5l3 3v9.4H4z" />
                <path d="M9 1.8v3h3" />
            </svg>
            <span>{fileName(file)}</span>
            {onRemove && <Remove name={fileName(file)} onRemove={onRemove} />}
        </li>
    );
}

/** What has been read of the images kept for sessions, by session and path; they never change. */
const shown = new Map<string, Promise<string>>();

function imageOf(sessionId: number, file: string): Promise<string> {
    const key = `${sessionId}\n${file}`;
    let image = shown.get(key);
    if (image === undefined) {
        image = api.sessionImage(sessionId, file);
        // One that could not be read is asked again next time: it may be there by then.
        image.catch(() => shown.delete(key));
        shown.set(key, image);
    }
    return image;
}

/**
 * An image pasted into the session and kept as its file. It takes its room at once, so that the
 * transcript does not move once it is read; one that cannot be read shows as the file it is.
 */
function KeptImage(props: { sessionId: number; file: string; onRemove: (() => void) | undefined }) {
    const { sessionId, file, onRemove } = props;
    const [source, setSource] = useState<string | null>(null);
    const [gone, setGone] = useState(false);
    useEffect(() => {
        let current = true;
        imageOf(sessionId, file).then(
            (found) => current && setSource(found),
            () => current && setGone(true),
        );
        return () => {
            current = false;
        };
    }, [sessionId, file]);
    if (gone) {
        return <FileChip file={file} onRemove={onRemove} />;
    }
    const open = () => void api.sessionImageOpen(sessionId, file).catch(useStore.getState().report);
    return (
        <li className="image-chip" title={`${fileName(file)}: open it in the viewer`}>
            <button
                type="button"
                className="image-open"
                aria-label={`Open ${fileName(file)}`}
                onClick={open}
            >
                {source && <img src={source} alt={fileName(file)} />}
            </button>
            {onRemove && <Remove name={fileName(file)} onRemove={onRemove} />}
        </li>
    );
}

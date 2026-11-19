import { fileName } from "../transcript";

interface Props {
    files: string[];
    /** Given where a file can be taken off again, as in a message still being written. */
    onRemove?: (file: string) => void;
}

/** Files the user points the agent at: each by its name, with its whole path on hover. */
export function FileChips({ files, onRemove }: Props) {
    if (files.length === 0) {
        return null;
    }
    return (
        <ul className="file-chips" aria-label="Attached files">
            {files.map((file) => (
                <li key={file} className="file-chip" title={file}>
                    <svg viewBox="0 0 16 16" aria-hidden="true">
                        <path d="M4 1.8h5l3 3v9.4H4z" />
                        <path d="M9 1.8v3h3" />
                    </svg>
                    <span>{fileName(file)}</span>
                    {onRemove && (
                        <button
                            type="button"
                            aria-label={`Remove ${fileName(file)}`}
                            title="Remove"
                            onClick={() => onRemove(file)}
                        >
                            ×
                        </button>
                    )}
                </li>
            ))}
        </ul>
    );
}

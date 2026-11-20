/**
 * A pod filling up: shown while a new task is being named and set up, which can take a while.
 * It takes the colour of the text around it.
 */
export function Incubating() {
    return (
        <svg className="incubating" viewBox="0 0 12 14" aria-hidden="true">
            <defs>
                <clipPath id="incubating-pod">
                    <path d="M6 1C3 1 1.5 5 1.5 8.2a4.5 4.5 0 0 0 9 0C10.5 5 9 1 6 1z" />
                </clipPath>
            </defs>
            <rect
                className="incubating-fill"
                x="0"
                y="0"
                width="12"
                height="14"
                clipPath="url(#incubating-pod)"
            />
            <path
                className="incubating-shell"
                d="M6 1C3 1 1.5 5 1.5 8.2a4.5 4.5 0 0 0 9 0C10.5 5 9 1 6 1z"
            />
        </svg>
    );
}

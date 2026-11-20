import { isSwarm } from "../appearance";
import { useStore } from "../store";

/**
 * Shown while a new task is being named and set up, which can take a while. The swarm's is a
 * sac that swells and then splits; the plain one a pod filling up. Both take the colour of the
 * text around them.
 */
export function Incubating() {
    const swarm = useStore((state) => isSwarm(state.settings?.style));
    return swarm ? <Sac /> : <Pod />;
}

function Sac() {
    return (
        <svg className="incubating incubating-sac" viewBox="0 0 12 14" aria-hidden="true">
            <path d="M.6 12.6c1.6-.2 2.6-.9 3.2-2M11.4 12.6c-1.6-.2-2.6-.9-3.2-2M6 11v1.9" />
            <path className="sac-body" d="M6 .8C3.4.8 2 4.2 2 7a4 4 0 0 0 8 0C10 4.2 8.6.8 6 .8z" />
            <path className="sac-split" d="M6 .6 5 3.4l1.7 1.5-1.3 2.3" />
        </svg>
    );
}

function Pod() {
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

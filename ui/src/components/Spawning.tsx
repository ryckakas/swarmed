/**
 * Something is on its way: one living cell, whose outline keeps shifting while it breathes and
 * its nucleus drifts. Shown wherever the app waits for an agent. It takes the colour of the text
 * around it.
 */
export function Spawning() {
    return (
        <span className="spawning" aria-hidden="true">
            <i />
        </span>
    );
}

/**
 * The colony at work: five cells like Spawning's, larger, swelling one after another. Shown
 * under the last line while an agent works and its tool calls are kept out of the transcript,
 * so that a long run of them does not look like a session that is stuck.
 */
export function Swarming() {
    return (
        <span className="swarming" aria-hidden="true">
            <i />
            <i />
            <i />
            <i />
            <i />
        </span>
    );
}

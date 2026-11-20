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

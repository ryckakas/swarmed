/**
 * Something is on its way: one cell that swells, divides into two, and closes up again. Shown
 * wherever the app waits for an agent. It takes the colour of the text around it.
 */
export function Spawning() {
    return (
        <span className="spawning" aria-hidden="true">
            <i />
            <i />
        </span>
    );
}

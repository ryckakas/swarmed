/**
 * What was read again, unless it holds just what was held: then what was held, so that nothing
 * drawn from it is drawn again. A terminal is built anew when its configuration changes, and a
 * file another process wrote has the window read all of its configuration again.
 */
export function unlessSame<T>(held: T, read: T): T {
    return JSON.stringify(held) === JSON.stringify(read) ? held : read;
}

import { useEffect, useRef } from "react";
import { useStore } from "../store";
import { useWords } from "../useWords";
import { About } from "./About";
import { CloseIcon, IconButton } from "./IconButton";
import { ShortcutLists } from "./Shortcuts";

/**
 * The About view or the keyboard shortcuts, over the window, from the menu bar, the version in
 * the corner or ⌘/. Closed by its button, Esc or a click beside it.
 */
export function BannerDialog() {
    const banner = useStore((state) => state.banner);
    const showBanner = useStore((state) => state.showBanner);
    const words = useWords();
    const dialog = useRef<HTMLDialogElement>(null);

    // A modal <dialog> keeps the keyboard inside it and closes on Esc by itself.
    useEffect(() => {
        if (banner && !dialog.current?.open) {
            dialog.current?.showModal();
        }
    }, [banner]);

    if (!banner) {
        return null;
    }
    const title = banner === "about" ? words.about : "Keyboard shortcuts";
    return (
        // biome-ignore lint/a11y/useKeyWithClickEvents: the keyboard's way out is Esc, which a modal dialog has by itself
        <dialog
            ref={dialog}
            className={`confirm banner banner-${banner}`}
            aria-label={title}
            onClose={() => showBanner(null)}
            // A click on the backdrop, outside the card, puts it away.
            onClick={(event) => event.target === dialog.current && showBanner(null)}
        >
            <header className="banner-head">
                {banner === "shortcuts" && <h2>Keyboard shortcuts</h2>}
                <IconButton label={`Close ${title.toLowerCase()}`} onClick={() => showBanner(null)}>
                    <CloseIcon />
                </IconButton>
            </header>
            <div className="banner-body">{banner === "about" ? <About /> : <ShortcutLists />}</div>
        </dialog>
    );
}

import { useStore } from "../store";
import { useWords } from "../useWords";
import { About } from "./About";
import { Dialog } from "./Dialog";
import { ShortcutLists } from "./Shortcuts";

/**
 * The About view or the keyboard shortcuts, over the window, from the menu bar, the version in
 * the corner or ⌘/. Nothing in them can be lost, so a click beside them closes them too.
 */
export function BannerDialog() {
    const banner = useStore((state) => state.banner);
    const showBanner = useStore((state) => state.showBanner);
    const words = useWords();

    if (!banner) {
        return null;
    }
    return (
        <Dialog
            title={banner === "about" ? words.about : words.shortcuts}
            size={banner === "about" ? "small" : "medium"}
            closeOnBackdrop={true}
            onClose={() => showBanner(null)}
        >
            {banner === "about" ? <About /> : <ShortcutLists />}
        </Dialog>
    );
}

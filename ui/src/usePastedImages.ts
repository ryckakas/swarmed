import { useEffect, useRef, useState } from "react";
import { api } from "./api";
import { imageFolder, imagesAmong, type PastedImage, readPasted, withImages } from "./images";
import { useStore } from "./store";

/** The folder every session's pasted images are in, once the core has said where it is. */
let root: string | null = null;
let asked: Promise<string> | null = null;

/**
 * The folder session `session`'s pasted images are kept in: a message's files that lie in it
 * show as images. `null` until the core has said where its folder is, and for no session.
 */
export function useImageFolder(session: number | undefined): string | null {
    const [known, setKnown] = useState(root);
    useEffect(() => {
        if (known !== null) {
            return;
        }
        asked ??= api.attachmentsRoot();
        let current = true;
        asked
            .then((found) => {
                root = found;
                if (current) {
                    setKnown(found);
                }
            })
            .catch(() => {
                // Without the folder, pasted images show as the files they are.
                asked = null;
            });
        return () => {
            current = false;
        };
    }, [known]);
    return session === undefined ? null : imageFolder(known, session);
}

/** The images among what a paste holds: as files, or as items of kind `file`. */
function imagesOfPaste(data: DataTransfer): File[] {
    const files = imagesAmong(data.files);
    if (files.length > 0) {
        return files;
    }
    const items = [...data.items].filter((item) => item.kind === "file");
    const found = items.map((item) => item.getAsFile()).filter((file) => file !== null);
    return imagesAmong(found);
}

/**
 * Taking images pasted into a message box: returns what the box calls with a paste's data,
 * which says whether the paste held any images. They are read and checked meanwhile, then
 * added to `images` as they stand by then; one that cannot be pasted is reported with why.
 */
export function usePastedImages(images: PastedImage[], onImages: (images: PastedImage[]) => void) {
    // The latest list and setter, for reading that ends after the render that started it.
    const latest = useRef({ images, onImages });
    latest.current = { images, onImages };
    return (data: DataTransfer): boolean => {
        const files = imagesOfPaste(data);
        if (files.length === 0) {
            return false;
        }
        void readPasted(files).then(({ images: added, refused }) => {
            const { report } = useStore.getState();
            for (const reason of refused) {
                report(reason);
            }
            if (added.length > 0) {
                const now = latest.current;
                now.onImages(withImages(now.images, added));
            }
        });
        return true;
    };
}

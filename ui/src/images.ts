// Images pasted into a message box. Until the message goes, one is kept in the draft, in the
// window's memory, as the rest of a draft is. When it goes, the core keeps it as a file of the
// session's own (`attachments/<session>/` in the data directory) and it is attached by its
// path like any file. Pure, so it can be tested; the core checks everything again.

/** The formats agents read, the only ones that can be pasted. */
export type ImageFormat = "png" | "jpeg" | "gif" | "webp";

/** The largest image that can be pasted, in megabytes, as the core keeps them. */
export const MAX_IMAGE_MB = 20;

const MAX_IMAGE_BYTES = MAX_IMAGE_MB * 1024 * 1024;

/** Why an image too large is refused, as the core says it. */
const TOO_LARGE = `an image of more than ${MAX_IMAGE_MB} MB cannot be attached`;

/** An image pasted into a message still being written. */
export interface PastedImage {
    /** Tells it apart from the others in the same draft. */
    key: number;
    format: ImageFormat;
    /** The image itself, in base64, as the core is handed it. */
    data: string;
}

/** What a pasted image turned out to be: one to keep, or why it cannot be. */
export type Checked = { format: ImageFormat } | { refused: string };

function startsWith(bytes: Uint8Array, start: number[], at = 0): boolean {
    return start.every((byte, index) => bytes[at + index] === byte);
}

function ascii(text: string): number[] {
    return [...text].map((char) => char.charCodeAt(0));
}

/** The format agents read that `bytes` are in, told by their first bytes, or `null`. */
function formatOf(bytes: Uint8Array): ImageFormat | null {
    if (startsWith(bytes, [0x89, ...ascii("PNG\r\n\x1a\n")])) {
        return "png";
    }
    if (startsWith(bytes, [0xff, 0xd8, 0xff])) {
        return "jpeg";
    }
    if (startsWith(bytes, ascii("GIF87a")) || startsWith(bytes, ascii("GIF89a"))) {
        return "gif";
    }
    if (startsWith(bytes, ascii("RIFF")) && startsWith(bytes, ascii("WEBP"), 8)) {
        return "webp";
    }
    return null;
}

const HEIF_BRANDS = ["heic", "heix", "hevc", "heim", "mif1", "msf1"];

/** What an image agents do not read is, as the core says it. */
function named(bytes: Uint8Array): string {
    const brand = String.fromCharCode(...bytes.slice(8, 12));
    const boxed = startsWith(bytes, ascii("ftyp"), 4);
    if (startsWith(bytes, ascii("II*\0")) || startsWith(bytes, ascii("MM\0*"))) {
        return "a TIFF image";
    }
    if (startsWith(bytes, ascii("BM"))) {
        return "a BMP image";
    }
    if (boxed && brand === "avif") {
        return "an AVIF image";
    }
    return boxed && HEIF_BRANDS.includes(brand) ? "a HEIC image" : "what was pasted";
}

/**
 * Whether `bytes` can be pasted: a format agents read, told by its first bytes and not by a
 * name, and not too large. Anything else is refused with the reason, in the core's words.
 */
export function checkImage(bytes: Uint8Array): Checked {
    if (bytes.length > MAX_IMAGE_BYTES) {
        return { refused: TOO_LARGE };
    }
    const format = formatOf(bytes);
    if (format === null) {
        const what = named(bytes);
        return {
            refused: `${what} cannot be attached: agents read only PNG, JPEG, GIF and WebP images`,
        };
    }
    return { format };
}

/** What a paste holds that might be an image: what its type says, not what it is. */
export function imagesAmong<T extends { type: string }>(files: Iterable<T>): T[] {
    return [...files].filter((file) => file.type.startsWith("image/"));
}

/** `bytes` in base64, a piece at a time: one call with all of them would overflow the stack. */
export function toBase64(bytes: Uint8Array): string {
    const pieces: string[] = [];
    for (let at = 0; at < bytes.length; at += 0x8000) {
        pieces.push(String.fromCharCode(...bytes.subarray(at, at + 0x8000)));
    }
    return btoa(pieces.join(""));
}

/** Where the window can show a pasted image from: its own data, as its policy allows. */
export function imageSource(image: Pick<PastedImage, "format" | "data">): string {
    return `data:image/${image.format};base64,${image.data}`;
}

/** The images of a draft with those `added`, each with a key no other has. */
export function withImages(
    images: PastedImage[],
    added: Omit<PastedImage, "key">[],
): PastedImage[] {
    let key = Math.max(0, ...images.map((image) => image.key));
    return [...images, ...added.map((image) => ({ ...image, key: ++key }))];
}

/** The folder session `session`'s pasted images are kept in, once the root is known. */
export function imageFolder(root: string | null, session: number): string | null {
    return root === null ? null : `${root.replace(/\/+$/, "")}/${session}`;
}

/**
 * Whether `file`, attached to a message, is an image pasted into the session whose folder is
 * `folder`: a file right in it, under a name the core gives. Anything else is a file like any.
 */
export function isPastedImage(file: string, folder: string | null): boolean {
    if (folder === null || !file.startsWith(`${folder}/`)) {
        return false;
    }
    return /^pasted-\d+\.(png|jpg|gif|webp)$/.test(file.slice(folder.length + 1));
}

/** What a paste's images turned out to be: those to add to the draft, and why others were not. */
export interface ReadPaste {
    images: Omit<PastedImage, "key">[];
    refused: string[];
}

/** Reads one pasted image and checks it; one too large is refused before it is read. */
async function readOne(file: Blob): Promise<Checked & { bytes?: Uint8Array }> {
    if (file.size > MAX_IMAGE_BYTES) {
        return { refused: TOO_LARGE };
    }
    const bytes = new Uint8Array(await file.arrayBuffer());
    return { ...checkImage(bytes), bytes };
}

/** Reads the images of a paste and checks each. The core checks them again when it keeps them. */
export async function readPasted(files: Blob[]): Promise<ReadPaste> {
    const read: ReadPaste = { images: [], refused: [] };
    for (const one of await Promise.all(files.map(readOne))) {
        if ("refused" in one) {
            read.refused.push(one.refused);
        } else if (one.bytes) {
            read.images.push({ format: one.format, data: toBase64(one.bytes) });
        }
    }
    return read;
}

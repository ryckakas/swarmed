// Images a tool returned are drawn small in the transcript, from a copy made smaller in the
// window: a screenshot of a Retina screen is megabytes, and a session can hold dozens of
// them. Only the small copy is held; the image is read whole again when it is shown large.

/** The longest side of an image in a strip under a call, in pixels: twice the room it takes. */
export const THUMBNAIL_PX = 320;

/** The longest side of the turn's last image at the end of its reply, in pixels. */
export const PREVIEW_PX = 960;

/**
 * The size an image of `width` by `height` pixels is drawn at as a copy whose longest side is
 * at most `most`: never larger than it is, and never nothing.
 */
export function thumbnailSize(
    width: number,
    height: number,
    most: number,
): { width: number; height: number } {
    const scale = Math.min(1, most / Math.max(width, height, 1));
    return {
        width: Math.max(1, Math.round(width * scale)),
        height: Math.max(1, Math.round(height * scale)),
    };
}

/** The type a smaller copy of the image at `source` is kept in: JPEG stays JPEG, the rest PNG. */
export function copyType(source: string): "image/jpeg" | "image/png" {
    return source.startsWith("data:image/jpeg") ? "image/jpeg" : "image/png";
}

/**
 * The image at `source`, a `data:` address, as a copy whose longest side is at most `most`.
 * One small enough already, or one that cannot be drawn, is handed back as it is.
 */
export async function shrink(source: string, most: number): Promise<string> {
    try {
        const image = new Image();
        image.src = source;
        await image.decode();
        const natural = { width: image.naturalWidth, height: image.naturalHeight };
        const { width, height } = thumbnailSize(natural.width, natural.height, most);
        const canvas = document.createElement("canvas");
        const context = width < natural.width ? canvas.getContext("2d") : null;
        if (context === null) {
            return source;
        }
        canvas.width = width;
        canvas.height = height;
        context.drawImage(image, 0, 0, width, height);
        return canvas.toDataURL(copyType(source));
    } catch {
        return source;
    }
}

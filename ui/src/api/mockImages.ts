import { checkImage, imageFolder, imageSource } from "../images";
import type { Api } from "./client";

/** Where the sample keeps pasted images, as the core would in the user's data directory. */
export const MOCK_ATTACHMENTS = "/Users/you/Library/Application Support/swarmed/attachments";

/** The bytes base64 stands for. */
function decoded(data: string): Uint8Array {
    return Uint8Array.from(atob(data), (char) => char.charCodeAt(0));
}

/**
 * Pasted images as the core keeps them, in memory: each checked, named by a number in the
 * session's own folder, and read back only from there, as are those the agent's tools
 * `returned`.
 */
export function mockImagesApi(
    returned: ReadonlyMap<string, string> = new Map(),
): Pick<Api, "sessionImagesKeep" | "sessionImage" | "sessionImageOpen" | "attachmentsRoot"> {
    const kept = new Map<string, string>();
    const counts = new Map<number, number>();
    const shown = (id: number, path: string): string => {
        const folder = imageFolder(MOCK_ATTACHMENTS, id);
        const inside = path.startsWith(`${folder}/`);
        const image = inside ? (kept.get(path) ?? returned.get(path)) : undefined;
        if (image === undefined) {
            throw new Error(`${path} is not an image kept for this session`);
        }
        return image;
    };
    return {
        sessionImagesKeep: async (id, images) => {
            const checked = images.map((data) => ({ data, checked: checkImage(decoded(data)) }));
            const refused = checked.find((image) => "refused" in image.checked);
            if (refused && "refused" in refused.checked) {
                throw new Error(refused.checked.refused);
            }
            return checked.map(({ data, checked }) => {
                const number = (counts.get(id) ?? 0) + 1;
                counts.set(id, number);
                const format = "format" in checked ? checked.format : "png";
                const extension = format === "jpeg" ? "jpg" : format;
                const path = `${imageFolder(MOCK_ATTACHMENTS, id)}/pasted-${number}.${extension}`;
                kept.set(path, imageSource({ format, data }));
                return path;
            });
        },
        sessionImage: async (id, path) => shown(id, path),
        // A browser has no viewer to open it in: the sample only checks it is the session's.
        sessionImageOpen: async (id, path) => void shown(id, path),
        attachmentsRoot: async () => MOCK_ATTACHMENTS,
    };
}

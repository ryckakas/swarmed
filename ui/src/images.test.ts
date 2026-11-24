import { describe, expect, it } from "vitest";
import {
    checkImage,
    imageFolder,
    imageSource,
    imagesAmong,
    isPastedImage,
    isTextWithItsPicture,
    MAX_IMAGE_MB,
    readPasted,
    toBase64,
    withImages,
} from "./images";

const bytes = (...parts: (string | number[])[]) =>
    new Uint8Array(
        parts.flatMap((part) =>
            typeof part === "string" ? [...part].map((char) => char.charCodeAt(0)) : part,
        ),
    );

const PNG = bytes([0x89], "PNG\r\n\x1a\n", [0, 0, 0, 13]);
const READ = "agents read only PNG, JPEG, GIF and WebP images";

describe("checkImage", () => {
    it("tells the formats agents read by their first bytes", () => {
        expect(checkImage(PNG)).toEqual({ format: "png" });
        expect(checkImage(bytes([0xff, 0xd8, 0xff, 0xe0]))).toEqual({ format: "jpeg" });
        expect(checkImage(bytes("GIF87a"))).toEqual({ format: "gif" });
        expect(checkImage(bytes("RIFF", [0, 0, 0, 0], "WEBPVP8 "))).toEqual({ format: "webp" });
    });

    it("refuses any other with what it is, as the core says it", () => {
        expect(checkImage(bytes("II*\0..."))).toEqual({
            refused: `a TIFF image cannot be attached: ${READ}`,
        });
        expect(checkImage(bytes("MM\0*..."))).toEqual({
            refused: `a TIFF image cannot be attached: ${READ}`,
        });
        expect(checkImage(bytes("BM..."))).toEqual({
            refused: `a BMP image cannot be attached: ${READ}`,
        });
        expect(checkImage(bytes([0, 0, 0, 24], "ftypheic"))).toEqual({
            refused: `a HEIC image cannot be attached: ${READ}`,
        });
        expect(checkImage(bytes([0, 0, 0, 24], "ftypavif"))).toEqual({
            refused: `an AVIF image cannot be attached: ${READ}`,
        });
        expect(checkImage(bytes("<svg"))).toEqual({
            refused: `what was pasted cannot be attached: ${READ}`,
        });
        expect(checkImage(new Uint8Array(MAX_IMAGE_MB * 1024 * 1024 + 1))).toEqual({
            refused: `an image of more than ${MAX_IMAGE_MB} MB cannot be attached`,
        });
    });
});

describe("pasted images", () => {
    it("are only the files of a paste whose type says image", () => {
        const files = [{ type: "image/png" }, { type: "text/plain" }, { type: "image/tiff" }];
        expect(imagesAmong(files)).toEqual([{ type: "image/png" }, { type: "image/tiff" }]);
    });

    it("are read, checked and given in base64, and those refused say why", async () => {
        const read = await readPasted([new Blob([PNG]), new Blob([bytes("II*\0")])]);
        expect(read).toEqual({
            images: [{ format: "png", data: btoa(String.fromCharCode(...PNG)) }],
            refused: [`a TIFF image cannot be attached: ${READ}`],
        });
    });

    it("are shown from their own data, as the window's policy allows", () => {
        expect(imageSource({ format: "png", data: "iVBORw0KGgo=" })).toBe(
            "data:image/png;base64,iVBORw0KGgo=",
        );
        // Large enough to need more than one piece.
        const large = new Uint8Array(100_000).map((_, index) => index % 251);
        expect(atob(toBase64(large))).toBe(String.fromCharCode(...large));
    });

    it("each get a key no other image of the draft has", () => {
        const one = withImages([], [{ format: "png", data: "a" }]);
        const two = withImages(one, [
            { format: "gif", data: "b" },
            { format: "png", data: "c" },
        ]);
        expect(two.map((image) => image.key)).toEqual([1, 2, 3]);
        expect(withImages(two.slice(1), [{ format: "png", data: "d" }]).at(-1)?.key).toBe(4);
    });
});

describe("isPastedImage", () => {
    const folder = imageFolder("/data/attachments/", 12);

    it("is a file right in the session's folder, named as the core names them", () => {
        expect(folder).toBe("/data/attachments/12");
        expect(isPastedImage("/data/attachments/12/pasted-1.png", folder)).toBe(true);
        expect(isPastedImage("/data/attachments/12/pasted-30.webp", folder)).toBe(true);
    });

    it("is nothing else: another session's, a file of the user's, or before the folder is known", () => {
        expect(isPastedImage("/data/attachments/120/pasted-1.png", folder)).toBe(false);
        expect(isPastedImage("/data/attachments/13/pasted-1.png", folder)).toBe(false);
        expect(isPastedImage("/data/attachments/12/deeper/pasted-1.png", folder)).toBe(false);
        expect(isPastedImage("/data/attachments/12/notes.txt", folder)).toBe(false);
        expect(isPastedImage("/Users/you/Desktop/pasted-1.png", folder)).toBe(false);
        expect(isPastedImage("/data/attachments/12/pasted-1.png", null)).toBe(false);
        expect(imageFolder(null, 12)).toBeNull();
    });
});

describe("isTextWithItsPicture", () => {
    it("takes cells copied from a spreadsheet as their text alone", () => {
        const types = ["text/plain", "text/html", "text/rtf", "Files"];
        expect(isTextWithItsPicture(types, "a\tb")).toBe(true);
    });

    it("leaves an image copied in a browser, which may come with its address, an image", () => {
        const types = ["text/plain", "text/html", "Files"];
        expect(isTextWithItsPicture(types, "https://example.com/cat.png")).toBe(false);
    });

    it("leaves a paste with no text an image", () => {
        expect(isTextWithItsPicture(["text/rtf", "Files"], "")).toBe(false);
        expect(isTextWithItsPicture(["Files"], "")).toBe(false);
    });
});

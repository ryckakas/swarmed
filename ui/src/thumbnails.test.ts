import { describe, expect, it } from "vitest";
import { copyType, thumbnailSize } from "./thumbnails";

describe("thumbnailSize", () => {
    it("makes the longest side fit and keeps the shape", () => {
        expect(thumbnailSize(1920, 1080, 320)).toEqual({ width: 320, height: 180 });
        expect(thumbnailSize(600, 2400, 320)).toEqual({ width: 80, height: 320 });
    });

    it("never draws an image larger than it is, nor as nothing", () => {
        expect(thumbnailSize(200, 100, 320)).toEqual({ width: 200, height: 100 });
        expect(thumbnailSize(10_000, 1, 320)).toEqual({ width: 320, height: 1 });
        expect(thumbnailSize(0, 0, 320)).toEqual({ width: 1, height: 1 });
    });
});

describe("copyType", () => {
    it("keeps a photo a photo and draws the rest losslessly", () => {
        expect(copyType("data:image/jpeg;base64,AAAA")).toBe("image/jpeg");
        expect(copyType("data:image/png;base64,AAAA")).toBe("image/png");
        expect(copyType("data:image/webp;base64,AAAA")).toBe("image/png");
    });
});

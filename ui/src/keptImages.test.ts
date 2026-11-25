import { describe, expect, it, vi } from "vitest";
import { forgetImages, keptImage, keptThumbnail } from "./keptImages";

describe("keptImage", () => {
    it("reads an image once, however often it is shown", async () => {
        const ask = vi.fn(async () => "data:image/png;base64,AAAA");
        await keptImage(1, "/a/pasted-1.png", ask);
        expect(await keptImage(1, "/a/pasted-1.png", ask)).toBe("data:image/png;base64,AAAA");
        expect(ask).toHaveBeenCalledTimes(1);
        forgetImages([1]);
    });

    it("keeps the images of two sessions apart", async () => {
        const ask = vi.fn(async (session: number) => `of ${session}`);
        expect(await keptImage(2, "/a/pasted-1.png", ask)).toBe("of 2");
        expect(await keptImage(3, "/a/pasted-1.png", ask)).toBe("of 3");
        forgetImages([2, 3]);
    });

    it("asks again for one that could not be read", async () => {
        const ask = vi
            .fn<(session: number, file: string) => Promise<string>>()
            .mockRejectedValueOnce(new Error("not there"))
            .mockResolvedValueOnce("there now");
        await expect(keptImage(4, "/a/pasted-1.png", ask)).rejects.toThrow("not there");
        expect(await keptImage(4, "/a/pasted-1.png", ask)).toBe("there now");
        forgetImages([4]);
    });
});

describe("forgetImages", () => {
    it("lets go of a session's images, and of no other's", async () => {
        const ask = vi.fn(async () => "image");
        await keptImage(5, "/a/pasted-1.png", ask);
        await keptImage(6, "/a/pasted-1.png", ask);
        forgetImages([5]);
        await keptImage(5, "/a/pasted-1.png", ask);
        await keptImage(6, "/a/pasted-1.png", ask);
        expect(ask).toHaveBeenCalledTimes(3);
        forgetImages([5, 6]);
    });
});

describe("keptThumbnail", () => {
    it("holds the smaller copy, made once for each size", async () => {
        const ask = vi.fn(async () => "whole");
        const shrink = vi.fn(async (whole: string, most: number) => `${whole} at ${most}`);
        expect(await keptThumbnail(7, "/a/tool-t1-1.png", 320, ask, shrink)).toBe("whole at 320");
        await keptThumbnail(7, "/a/tool-t1-1.png", 320, ask, shrink);
        expect(await keptThumbnail(7, "/a/tool-t1-1.png", 960, ask, shrink)).toBe("whole at 960");
        expect(shrink).toHaveBeenCalledTimes(2);
        // The whole image is not among those held: shown large, it is read again.
        await keptImage(7, "/a/tool-t1-1.png", ask);
        expect(ask).toHaveBeenCalledTimes(3);
        forgetImages([7]);
        await keptThumbnail(7, "/a/tool-t1-1.png", 320, ask, shrink);
        expect(shrink).toHaveBeenCalledTimes(3);
        forgetImages([7]);
    });
});

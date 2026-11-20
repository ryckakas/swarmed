import { describe, expect, it } from "vitest";
import { browserLabel } from "./TaskPanel";

describe("browserLabel", () => {
    it("names no address when there is only one to open", () => {
        expect(browserLabel("http://localhost:3000/", 1)).toBe("Open in browser");
    });

    it("says which address when a stack has several", () => {
        expect(browserLabel("http://localhost:3000/", 2)).toBe("Open localhost:3000");
        expect(browserLabel("https://one.localhost/admin", 2)).toBe("Open one.localhost/admin");
    });
});

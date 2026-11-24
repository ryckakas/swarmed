import { describe, expect, it } from "vitest";
import { unlessSame } from "./unchanged";

describe("what is read again", () => {
    it("keeps what was held when nothing in it changed", () => {
        const held = { font_size: 13, colours: { dark: { red: "#ff0000" } } };
        const read = { font_size: 13, colours: { dark: { red: "#ff0000" } } };
        expect(unlessSame(held, read)).toBe(held);
    });

    it("takes what was read when anything in it changed", () => {
        const held = { font_size: 13, colours: { dark: { red: "#ff0000" } } };
        const read = { font_size: 13, colours: { dark: { red: "#ee0000" } } };
        expect(unlessSame(held, read)).toBe(read);
        expect(unlessSame(null, read)).toBe(read);
    });
});

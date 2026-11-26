import { describe, expect, it } from "vitest";
import { hasLinks, saidLinks } from "./saidLinks";

/** An address in the pieces, written as `shown` and leading to `address`. */
function link(shown: string, address = shown) {
    return { address, shown };
}

describe("saidLinks", () => {
    it("leaves words without an address as they are", () => {
        expect(saidLinks("Fix the login form.")).toEqual(["Fix the login form."]);
        expect(saidLinks("")).toEqual([]);
        expect(hasLinks(saidLinks("Fix the login form."))).toBe(false);
    });

    it("finds a web address among words", () => {
        const pieces = saidLinks("See https://example.com/a?b=1#c for the log");
        expect(pieces).toEqual(["See ", link("https://example.com/a?b=1#c"), " for the log"]);
        expect(hasLinks(pieces)).toBe(true);
    });

    it("takes http as it is, in any case", () => {
        expect(saidLinks("http://localhost:3000/login")).toEqual([
            link("http://localhost:3000/login"),
        ]);
        expect(saidLinks("HTTPS://Example.com")).toEqual([link("HTTPS://Example.com")]);
    });

    it("opens an address that starts with www. on https", () => {
        expect(saidLinks("on www.example.com/report today")).toEqual([
            "on ",
            link("www.example.com/report", "https://www.example.com/report"),
            " today",
        ]);
    });

    it("leaves the punctuation that ends a sentence out", () => {
        for (const end of [".", ",", ";", ":", "!", "?", "'", "...", "?!"]) {
            expect(saidLinks(`Look at https://example.com${end}`)).toEqual([
                "Look at ",
                link("https://example.com"),
                end,
            ]);
        }
    });

    it("keeps punctuation inside an address", () => {
        expect(saidLinks("https://example.com/a.b,c;d:e!f?g=h.")).toEqual([
            link("https://example.com/a.b,c;d:e!f?g=h"),
            ".",
        ]);
    });

    it("leaves out a closing bracket that has no opening one inside the address", () => {
        expect(saidLinks("(see https://example.com/a)")).toEqual([
            "(see ",
            link("https://example.com/a"),
            ")",
        ]);
        expect(saidLinks("[https://example.com/a]")).toEqual([
            "[",
            link("https://example.com/a"),
            "]",
        ]);
        expect(saidLinks("(https://example.com).")).toEqual([
            "(",
            link("https://example.com"),
            ").",
        ]);
    });

    it("keeps brackets the address opens and closes itself", () => {
        expect(saidLinks("(see https://example.com/a_(b))")).toEqual([
            "(see ",
            link("https://example.com/a_(b)"),
            ")",
        ]);
        expect(saidLinks("https://en.wikipedia.org/wiki/Monad_(functional_programming).")).toEqual([
            link("https://en.wikipedia.org/wiki/Monad_(functional_programming)"),
            ".",
        ]);
        expect(saidLinks("http://[::1]:8080/health")).toEqual([link("http://[::1]:8080/health")]);
    });

    it("finds every address in a message", () => {
        const text =
            "Compare https://a.example.com/1 with www.b.example.com,\nthen http://c.example.com.";
        expect(saidLinks(text)).toEqual([
            "Compare ",
            link("https://a.example.com/1"),
            " with ",
            link("www.b.example.com", "https://www.b.example.com"),
            ",\nthen ",
            link("http://c.example.com"),
            ".",
        ]);
    });

    it("ends an address at a quote mark or an angle bracket", () => {
        expect(saidLinks('"https://example.com"')).toEqual(['"', link("https://example.com"), '"']);
        expect(saidLinks("<https://example.com>")).toEqual(["<", link("https://example.com"), ">"]);
    });

    it("finds no address inside a word, a path or a mail address", () => {
        for (const text of [
            "xhttps://example.com",
            "src/www.config.js",
            "ann@www.example.com",
            "awww.example.com",
            "re.www.example.com",
        ]) {
            expect(saidLinks(text)).toEqual([text]);
        }
    });

    it("finds no address in a start with nothing after it", () => {
        for (const text of ["https://", "https://.", "www.", "www..", "see http://)"]) {
            expect(hasLinks(saidLinks(text))).toBe(false);
        }
    });

    it("leaves tokens as they are, whatever their labels hold", () => {
        const text =
            'See [@42 Check https://example.com/a] and [#142 www.example.com fix], [Element 3: link to https://example.com/b], [Quote #1: "on https://example.com/c"] and [Pasted text #2 +20 lines] then https://example.com/d';
        expect(saidLinks(text)).toEqual([
            'See [@42 Check https://example.com/a] and [#142 www.example.com fix], [Element 3: link to https://example.com/b], [Quote #1: "on https://example.com/c"] and [Pasted text #2 +20 lines] then ',
            link("https://example.com/d"),
        ]);
    });

    it("finds an address right beside a token", () => {
        expect(saidLinks("[@42 Login]https://example.com [#7]")).toEqual([
            "[@42 Login]",
            link("https://example.com"),
            " [#7]",
        ]);
    });

    it("keeps an address a token cuts short apart from the token", () => {
        expect(saidLinks("https://example.com/[@42 Login]")).toEqual([
            link("https://example.com/"),
            "[@42 Login]",
        ]);
    });
});

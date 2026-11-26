import { describe, expect, it } from "vitest";
import { drawnApart, saidPieces } from "./said";

/** An address in the pieces, written as `shown` and leading to `address`. */
function link(shown: string, address = shown) {
    return { address, shown };
}

/** A path in the pieces, written as it is with nothing after it. */
function path(shown: string) {
    return { path: shown, shown };
}

describe("saidPieces", () => {
    it("leaves words without an address as they are", () => {
        expect(saidPieces("Fix the login form.")).toEqual(["Fix the login form."]);
        expect(saidPieces("")).toEqual([]);
        expect(drawnApart(saidPieces("Fix the login form."))).toBe(false);
    });

    it("finds a web address among words", () => {
        const pieces = saidPieces("See https://example.com/a?b=1#c for the log");
        expect(pieces).toEqual(["See ", link("https://example.com/a?b=1#c"), " for the log"]);
        expect(drawnApart(pieces)).toBe(true);
    });

    it("takes http as it is, in any case", () => {
        expect(saidPieces("http://localhost:3000/login")).toEqual([
            link("http://localhost:3000/login"),
        ]);
        expect(saidPieces("HTTPS://Example.com")).toEqual([link("HTTPS://Example.com")]);
    });

    it("opens an address that starts with www. on https", () => {
        expect(saidPieces("on www.example.com/report today")).toEqual([
            "on ",
            link("www.example.com/report", "https://www.example.com/report"),
            " today",
        ]);
    });

    it("leaves the punctuation that ends a sentence out", () => {
        for (const end of [".", ",", ";", ":", "!", "?", "'", "...", "?!"]) {
            expect(saidPieces(`Look at https://example.com${end}`)).toEqual([
                "Look at ",
                link("https://example.com"),
                end,
            ]);
        }
    });

    it("keeps punctuation inside an address", () => {
        expect(saidPieces("https://example.com/a.b,c;d:e!f?g=h.")).toEqual([
            link("https://example.com/a.b,c;d:e!f?g=h"),
            ".",
        ]);
    });

    it("leaves out a closing bracket that has no opening one inside the address", () => {
        expect(saidPieces("(see https://example.com/a)")).toEqual([
            "(see ",
            link("https://example.com/a"),
            ")",
        ]);
        expect(saidPieces("[https://example.com/a]")).toEqual([
            "[",
            link("https://example.com/a"),
            "]",
        ]);
        expect(saidPieces("(https://example.com).")).toEqual([
            "(",
            link("https://example.com"),
            ").",
        ]);
    });

    it("keeps brackets the address opens and closes itself", () => {
        expect(saidPieces("(see https://example.com/a_(b))")).toEqual([
            "(see ",
            link("https://example.com/a_(b)"),
            ")",
        ]);
        expect(saidPieces("https://en.wikipedia.org/wiki/Monad_(functional_programming).")).toEqual(
            [link("https://en.wikipedia.org/wiki/Monad_(functional_programming)"), "."],
        );
        expect(saidPieces("http://[::1]:8080/health")).toEqual([link("http://[::1]:8080/health")]);
    });

    it("finds every address in a message", () => {
        const text =
            "Compare https://a.example.com/1 with www.b.example.com,\nthen http://c.example.com.";
        expect(saidPieces(text)).toEqual([
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
        expect(saidPieces('"https://example.com"')).toEqual([
            '"',
            link("https://example.com"),
            '"',
        ]);
        expect(saidPieces("<https://example.com>")).toEqual([
            "<",
            link("https://example.com"),
            ">",
        ]);
    });

    it("finds no address inside a word, a path or a mail address", () => {
        for (const text of [
            "xhttps://example.com",
            "ann@www.example.com",
            "awww.example.com",
            "re.www.example.com",
        ]) {
            expect(saidPieces(text)).toEqual([text]);
        }
        expect(saidPieces("src/www.config.js")).toEqual([path("src/www.config.js")]);
    });

    it("finds no address in a start with nothing after it", () => {
        for (const text of ["https://", "https://.", "www.", "www..", "see http://)"]) {
            expect(drawnApart(saidPieces(text))).toBe(false);
        }
    });

    it("leaves tokens as they are, whatever their labels hold", () => {
        const text =
            'See [@42 Check https://example.com/a] and [#142 www.example.com fix], [Element 3: link to https://example.com/b], [Quote #1: "on https://example.com/c"] and [Pasted text #2 +20 lines] then https://example.com/d';
        expect(saidPieces(text)).toEqual([
            'See [@42 Check https://example.com/a] and [#142 www.example.com fix], [Element 3: link to https://example.com/b], [Quote #1: "on https://example.com/c"] and [Pasted text #2 +20 lines] then ',
            link("https://example.com/d"),
        ]);
    });

    it("finds an address right beside a token", () => {
        expect(saidPieces("[@42 Login]https://example.com [#7]")).toEqual([
            "[@42 Login]",
            link("https://example.com"),
            " [#7]",
        ]);
    });

    it("keeps an address a token cuts short apart from the token", () => {
        expect(saidPieces("https://example.com/[@42 Login]")).toEqual([
            link("https://example.com/"),
            "[@42 Login]",
        ]);
    });
});

describe("code in saidPieces", () => {
    it("draws code without its backticks", () => {
        const pieces = saidPieces("Call `task_open_file` from the panel.");
        expect(pieces).toEqual(["Call ", { code: "task_open_file" }, " from the panel."]);
        expect(drawnApart(pieces)).toBe(true);
    });

    it("finds every span, and one of two backticks that holds one", () => {
        expect(saidPieces("`a` and ``b`c``")).toEqual([{ code: "a" }, " and ", { code: "b`c" }]);
        expect(saidPieces("`` `x` ``")).toEqual([{ code: "`x`" }]);
    });

    it("leaves a backtick with no other on its line as it is", () => {
        for (const text of ["it's `done", "a ` b", "`a\nb`", "``a`", "` `"]) {
            expect(saidPieces(text)).toEqual([text]);
        }
    });

    it("leaves a code block typed as one as it is", () => {
        const text = "Run this:\n```\nnpm test\n```";
        expect(saidPieces(text)).toEqual([text]);
    });

    it("keeps an address or a path between backticks code", () => {
        expect(saidPieces("`https://example.com/a` and `ui/src/setup.ts:12`")).toEqual([
            { code: "https://example.com/a" },
            " and ",
            { code: "ui/src/setup.ts:12" },
        ]);
    });

    it("finds an address and a path beside code and beside each other", () => {
        expect(
            saidPieces("See https://example.com/a ui/src/setup.ts:12, then `pnpm test`."),
        ).toEqual([
            "See ",
            link("https://example.com/a"),
            " ",
            { path: "ui/src/setup.ts", shown: "ui/src/setup.ts:12", line: 12 },
            ", then ",
            { code: "pnpm test" },
            ".",
        ]);
    });

    it("leaves the path in an address to the address", () => {
        expect(saidPieces("https://github.com/o/r/blob/main/ui/src/setup.ts")).toEqual([
            link("https://github.com/o/r/blob/main/ui/src/setup.ts"),
        ]);
    });

    it("leaves tokens as they are, whatever their labels hold", () => {
        const text = "[@42 Fix `ui/src/a.ts`] and [#7 crates/core/src/files.rs] ";
        expect(saidPieces(`${text}\`b\``)).toEqual([text, { code: "b" }]);
    });

    it("leaves headings, emphasis and lists as they were typed", () => {
        const text = "# Plan\n\n- **first** step\n- _second_ step\n1. third";
        expect(saidPieces(text)).toEqual([text]);
    });
});

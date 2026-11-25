import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { defaultSchema } from "rehype-sanitize";
import { describe, expect, it } from "vitest";
import { UntrustedMarkdown } from "./components/Markdown";
import { htmlShown, ISSUE_SCHEMA, issueLeavesOut, leavesOut } from "./issueHtml";

/** Issue text as the window draws it, through the component the Issues view uses. */
const drawn = (text: string) => renderToStaticMarkup(createElement(UntrustedMarkdown, { text }));

describe("ISSUE_SCHEMA", () => {
    it("narrows the default, which follows GitHub's, and keeps no style, class or id", () => {
        const all = new Set(defaultSchema.tagNames);
        expect(ISSUE_SCHEMA.tagNames?.every((tag) => all.has(tag))).toBe(true);
        expect(ISSUE_SCHEMA.attributes?.["*"]).toBeUndefined();
        const named = Object.values(ISSUE_SCHEMA.attributes ?? {})
            .flat()
            .map((rule) => (typeof rule === "string" ? rule : rule[0]));
        expect(named).not.toContain("style");
        expect(named).not.toContain("id");
        expect(named).not.toContain("name");
        // The one class: a code block's language, which colouring it needs.
        expect(
            Object.keys(ISSUE_SCHEMA.attributes ?? {}).filter((tag) =>
                ISSUE_SCHEMA.attributes?.[tag]?.some((rule) => rule[0] === "className"),
            ),
        ).toEqual(["code"]);
    });
});

describe("drawing issue text", () => {
    it("never runs a script", () => {
        const html = drawn('Before <script>alert("run")</script> after');
        expect(html).not.toContain("<script");
        expect(html).not.toContain("alert");
        expect(html).toContain("Before");
    });

    it("drops event handlers, styles, classes and ids", () => {
        const html = drawn('<b onclick="alert(1)" style="color:red" class="x" id="y">bold</b>');
        expect(html).toBe('<div class="prose"><p><b>bold</b></p></div>');
    });

    it("draws an image as a link to it, never loaded, whatever it carries", () => {
        const html = drawn('<img src="https://example.com/a.png" alt="A cat" onerror="alert(1)">');
        expect(html).not.toContain("<img");
        expect(html).not.toContain("onerror");
        expect(html).toContain('<a href="https://example.com/a.png">[Image: A cat]</a>');
        expect(drawn("![A dog](https://example.com/d.png)")).toContain(
            '<a href="https://example.com/d.png">[Image: A dog]</a>',
        );
    });

    it("leaves out frames, forms, drawings and what they hold", () => {
        const html = drawn(
            [
                '<iframe src="https://example.com">framed</iframe>',
                "",
                '<form action="https://example.com"><input type="text" name="q" value="v">sent</form>',
                "",
                "<svg><text>drawn</text></svg><math><mi>x</mi></math><style>p{}</style>",
            ].join("\n"),
        );
        for (const gone of [
            "<iframe",
            "framed",
            "<form",
            "action",
            'type="text"',
            "value",
            "drawn",
        ]) {
            expect(html).not.toContain(gone);
        }
        expect(html).not.toMatch(/<(svg|math|style)/);
        // What a form says is text; a field in it can only be the disabled box of a task list.
        expect(html).toContain("sent");
        expect(html).toContain('<input disabled="" type="checkbox"/>');
    });

    it("keeps no address that is not on the web or for mail", () => {
        const html = drawn(
            '<a href="javascript:alert(1)">one</a> [two](javascript:alert(2)) <a href="data:text/html,x">three</a>',
        );
        expect(html).not.toContain("javascript:");
        expect(html).not.toContain("data:");
        expect(drawn('<a href="mailto:a@example.com">mail</a>')).toContain(
            'href="mailto:a@example.com"',
        );
    });

    it("keeps the HTML GitHub draws to format text", () => {
        const html = drawn(
            [
                "<details open><summary>Logs</summary>",
                "",
                "Folded **text**",
                "",
                "</details>",
                "",
                "Press <kbd>Ctrl</kbd>+<kbd>C</kbd>, <b>bold</b>, <i>slanted</i>, H<sub>2</sub>O, x<sup>2</sup>, <s>gone</s>, <del>out</del><br>next",
            ].join("\n"),
        );
        expect(html).toContain('<details open=""><summary>Logs</summary>');
        expect(html).toContain("<strong>text</strong>");
        expect(html).toContain("</details>");
        for (const kept of ["<kbd>Ctrl</kbd>", "<b>bold</b>", "<i>slanted</i>", "<sub>2</sub>"]) {
            expect(html).toContain(kept);
        }
        for (const kept of ["<sup>2</sup>", "<s>gone</s>", "<del>out</del>", "<br/>"]) {
            expect(html).toContain(kept);
        }
    });

    it("hides an HTML comment, as GitHub does", () => {
        expect(drawn("Shown <!-- hidden --> too")).not.toContain("hidden");
    });

    it("keeps Markdown's own: task lists, tables and coloured code", () => {
        expect(drawn("- [x] done")).toContain('<input type="checkbox" disabled="" checked=""/>');
        expect(drawn("| a |\n| - |\n| b |")).toContain("<td>b</td>");
        const code = drawn("```js\nconst a = 1;\n```");
        expect(code).toContain('class="hljs language-js"');
        expect(code).toContain('class="hljs-keyword"');
    });

    it("leaves HTML in code as the text it is", () => {
        expect(drawn("`<script>`")).toContain("<code>&lt;script&gt;</code>");
        expect(drawn("```html-view\n<h1>x</h1>\n```")).toContain("&lt;h1&gt;x&lt;/h1&gt;");
    });
});

describe("htmlShown", () => {
    it("is so for the HTML that formats, written in any case", () => {
        expect(htmlShown("<details open><summary>a</summary>b</details>")).toBe(true);
        expect(htmlShown("<B>x</B> <KBD>y</KBD> <br/> <br />")).toBe(true);
        expect(htmlShown('<img src="https://example.com/a.png" alt="a">')).toBe(true);
        expect(htmlShown('<a href="https://example.com/?a=1&b=2">x</a>')).toBe(true);
        expect(htmlShown('<code class="language-js">x</code>')).toBe(true);
        expect(htmlShown('<td align="center" colspan="2">x</td>')).toBe(true);
        expect(htmlShown("a < b and c > d")).toBe(true);
    });

    it("is not for anything cleaning drops or changes", () => {
        for (const html of [
            "<!-- hidden -->",
            "<!DOCTYPE html>",
            "<?xml hidden?>",
            "<script>x</script>",
            "<div>x</div>",
            '<b onclick="x">y</b>',
            '<b style="color:red">y</b>',
            '<b class="x">y</b>',
            '<code class="x">y</code>',
            '<b id="x">y</b>',
            '<a href="javascript:x">y</a>',
            '<a href="jav&#x61;script:x">y</a>',
            '<a href=" javascript:x">y</a>',
            // Cleaning compares schemes as written, so this one loses its address.
            '<a href="HTTPS://example.com">y</a>',
            '<a href="https://example.com" href="javascript:x">y</a>',
            '<img src="x" width="3">',
            '<input type="checkbox" disabled>',
        ]) {
            expect(htmlShown(html), html).toBe(false);
        }
    });

    it("is not for HTML that cannot be read whole", () => {
        for (const html of [
            '<b title="never closed',
            "</b hidden>",
            "<b/onclick=x>",
            "<x-y>",
            "</>",
        ]) {
            expect(htmlShown(html), html).toBe(false);
        }
    });

    it("reads a long run of attributes quickly", () => {
        const started = performance.now();
        expect(htmlShown(`<b${" a".repeat(20_000)}`)).toBe(false);
        expect(htmlShown(`<b ${"a".repeat(20_000)}`)).toBe(false);
        expect(performance.now() - started).toBeLessThan(500);
    });
});

describe("leavesOut", () => {
    it("is not so for Markdown and the HTML that formats", () => {
        expect(leavesOut("**Steps**\n\n1. Open `/login`\n2. [Log in](https://example.com)")).toBe(
            false,
        );
        expect(leavesOut("<details>\n<summary>Logs</summary>\n\n```\nlog\n```\n\n</details>")).toBe(
            false,
        );
        expect(leavesOut("Press <kbd>Esc</kbd>.")).toBe(false);
        expect(leavesOut("![A cat](https://example.com/a.png)")).toBe(false);
        expect(leavesOut("See [the docs][1].\n\n[1]: https://example.com")).toBe(false);
        expect(leavesOut("A note[^1].\n\n[^1]: It says this.")).toBe(false);
    });

    it("is so for an HTML comment, as GitHub hides it", () => {
        expect(leavesOut("Shown <!-- hidden -->")).toBe(true);
        expect(leavesOut("<!--\nA template's hint\n-->\n\nText")).toBe(true);
    });

    it("is so for tags and attributes cleaning drops", () => {
        expect(leavesOut('<script>alert("x")</script>')).toBe(true);
        expect(leavesOut('<img src="x" onerror="alert(1)">')).toBe(true);
        expect(leavesOut('<p style="color:red">x</p>')).toBe(true);
        expect(leavesOut('<iframe src="https://example.com"></iframe>')).toBe(true);
        expect(leavesOut('<form action="https://example.com">x</form>')).toBe(true);
    });

    it("is so for an address that goes, a title, and a definition nothing uses", () => {
        expect(leavesOut("[x](javascript:alert(1))")).toBe(true);
        expect(leavesOut('[x](https://example.com "A title")')).toBe(true);
        expect(leavesOut("[//]: # (A Markdown comment)")).toBe(true);
        expect(leavesOut("[^1]: A note nothing points at.")).toBe(true);
    });

    it("is so for words after a code block's language, which are not drawn", () => {
        expect(leavesOut("```js these words are not drawn\nconst a = 1;\n```")).toBe(true);
        expect(leavesOut("~~~ text more words\nx\n~~~")).toBe(true);
        expect(leavesOut("```js\nconst a = 1;\n```")).toBe(false);
    });

    it("is so for a table's cells beyond its head's, which are not drawn", () => {
        expect(leavesOut("| a |\n| - |\n| b | a cell too many |")).toBe(true);
        expect(leavesOut("| a | b |\n| - | - |\n| c | d |\n| e |")).toBe(false);
    });

    it("counts nothing written in code", () => {
        expect(leavesOut("`<script>` and `<!-- -->`")).toBe(false);
        expect(leavesOut("```html\n<!-- a comment -->\n<div onclick=x></div>\n```")).toBe(false);
        expect(leavesOut("    <script>indented code</script>")).toBe(false);
    });
});

describe("issueLeavesOut", () => {
    const comment = (body: string, hidden: string | null = null) => ({
        author: "sam",
        body,
        created_at: "2026-11-24T12:00:00Z",
        hidden,
    });

    it("looks at what the issue and the comments GitHub shows say", () => {
        expect(issueLeavesOut({ body: "Fine", comments: [comment("Fine too")] })).toBe(false);
        expect(issueLeavesOut({ body: "<!-- x -->", comments: [] })).toBe(true);
        expect(issueLeavesOut({ body: "Fine", comments: [comment("<!-- x -->")] })).toBe(true);
        // A comment GitHub hides says so in place of its words, which the core leaves out.
        expect(issueLeavesOut({ body: "Fine", comments: [comment("<!-- x -->", "spam")] })).toBe(
            false,
        );
    });
});

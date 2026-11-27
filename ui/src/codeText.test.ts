import { type ComponentProps, createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import ReactMarkdown from "react-markdown";
import rehypeHighlight from "rehype-highlight";
import { describe, expect, it } from "vitest";
import { copiedText, textOf } from "./codeText";

/** What the blocks drawn last copied, in order. */
let copied: string[] = [];

/** A code block that only notes what its copy button would copy. */
function Capture({ children }: ComponentProps<"pre">) {
    copied.push(copiedText(children));
    return null;
}

/** What the copy button of each code block in `markdown` copies, drawn as a reply is. */
function copiedFrom(markdown: string): string[] {
    copied = [];
    renderToStaticMarkup(
        createElement(
            ReactMarkdown,
            { rehypePlugins: [rehypeHighlight], components: { pre: Capture } },
            markdown,
        ),
    );
    return copied;
}

describe("textOf", () => {
    it("reads the words of elements however deeply they are wrapped", () => {
        const node = createElement("code", null, [
            "let ",
            createElement("span", { key: 1, className: "hljs-title" }, [
                createElement("b", { key: 2 }, "total"),
            ]),
            " = ",
            7,
        ]);
        expect(textOf(node)).toBe("let total = 7");
    });

    it("reads nothing from what has no words", () => {
        expect(textOf(null)).toBe("");
        expect(textOf(true)).toBe("");
        expect(textOf(undefined)).toBe("");
    });
});

describe("copiedText", () => {
    it("is the code as written: no fence, no language, no colouring", () => {
        const markdown = ["```ts", 'const greeting: string = "hi";', "greet(greeting);", "```"];
        expect(copiedFrom(markdown.join("\n"))).toEqual([
            'const greeting: string = "hi";\ngreet(greeting);',
        ]);
    });

    it("leaves out the one newline Markdown adds, and only that one", () => {
        expect(copiedFrom("```\nls -la\n```")).toEqual(["ls -la"]);
        expect(copiedFrom("```\nls -la\n\n```")).toEqual(["ls -la\n"]);
    });

    it("keeps the indentation and the blank lines inside", () => {
        const markdown = ["```py", "def f():", "", "    return 1", "```"].join("\n");
        expect(copiedFrom(markdown)).toEqual(["def f():\n\n    return 1"]);
    });

    it("copies a block indented rather than fenced", () => {
        expect(copiedFrom("Run:\n\n    cargo test\n    cargo fmt\n")).toEqual([
            "cargo test\ncargo fmt",
        ]);
    });

    it("copies what has arrived of a block still being written", () => {
        expect(copiedFrom("```sh\nnpm install\nnpm ru")).toEqual(["npm install\nnpm ru"]);
    });

    it("copies nothing from an empty block", () => {
        expect(copiedFrom("```\n```")).toEqual([""]);
    });

    it("gives each block its own code", () => {
        expect(copiedFrom("```\none\n```\n\nThen:\n\n```\ntwo\n```")).toEqual(["one", "two"]);
    });
});

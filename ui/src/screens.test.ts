import { describe, expect, it } from "vitest";
import { barWidth, escaped, resetsLine, screenHtml } from "./screens";

const plain = (time: Date) => time.toISOString().slice(0, 16);

describe("a screen as HTML", () => {
    it("shows each part's rows, with a bar where a row is a measure", () => {
        const html = screenHtml(
            {
                note: "On the team plan.",
                sections: [
                    {
                        title: "Plan limits",
                        rows: [
                            {
                                label: "Current session",
                                value: "40% used",
                                share: 40,
                                resets_at: "2026-11-21T19:59:59+00:00",
                                note: "",
                            },
                            { label: "Version", value: "2.1.330" },
                        ],
                    },
                    { title: "", rows: [{ label: "", value: "A remark." }] },
                ],
            },
            plain,
        );
        expect(html).toContain('<p class="note">On the team plan.</p>');
        expect(html).toContain("<h2>Plan limits</h2>");
        expect(html).toContain('<i style="width:40%"></i>');
        expect(html).toContain('<div class="sub">Resets 2026-11-21T19:59</div>');
        expect(html).toContain('<span class="value">2.1.330</span>');
        // A row with no name is a remark across the whole width, and a part with no heading has none.
        expect(html).toContain('<div class="remark">A remark.</div>');
        expect(html.match(/<h2>/g)).toHaveLength(1);
        expect(html.match(/class="bar"/g)).toHaveLength(1);
    });

    it("writes what the agent said as text, never as markup", () => {
        const html = screenHtml({
            note: "<script>alert(1)</script>",
            sections: [
                {
                    title: "<b>T</b>",
                    rows: [{ label: '"><img src=x>', value: "a & b", note: "<i>n</i>" }],
                },
            ],
        });
        const body = html.slice(html.indexOf("</style>"));
        expect(body).not.toContain("<script>");
        expect(body).not.toContain("<img");
        expect(body).not.toContain("<b>");
        expect(body).toContain("&lt;script&gt;alert(1)&lt;/script&gt;");
        expect(body).toContain("a &amp; b");
        expect(body).toContain("&lt;i&gt;n&lt;/i&gt;");
        expect(escaped(`'"`)).toBe("&#39;&quot;");
    });

    it("keeps a bar within its track and leaves out a share that is no number", () => {
        expect(barWidth(40.4)).toBe(40);
        expect(barWidth(250)).toBe(100);
        expect(barWidth(-3)).toBe(0);
        expect(barWidth(null)).toBeNull();
        expect(barWidth(undefined)).toBeNull();
        expect(barWidth(Number.NaN)).toBeNull();
        // Whatever arrives as a share, only a number reaches the style it is written into.
        const share = "40%;background:url(x)" as unknown as number;
        const html = screenHtml({
            note: "",
            sections: [{ title: "", rows: [{ label: "a", value: "b", share }] }],
        });
        expect(html).not.toContain("url(x)");
    });

    it("says when a measure starts over only for a time it can read", () => {
        expect(resetsLine("2026-11-21T19:59:59+00:00", plain)).toBe("Resets 2026-11-21T19:59");
        expect(resetsLine("soon", plain)).toBe("");
        expect(resetsLine(null, plain)).toBe("");
        expect(resetsLine(undefined, plain)).toBe("");
    });

    it("is only its remark when there is nothing else", () => {
        const html = screenHtml({ note: "No hooks are set.", sections: [] });
        expect(html).toContain("No hooks are set.");
        expect(html).not.toContain("<section>");
    });
});

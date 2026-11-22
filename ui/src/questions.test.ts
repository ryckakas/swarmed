import { describe, expect, it } from "vitest";
import {
    allAnswered,
    allSkipped,
    answersOf,
    choose,
    commandAnswersOf,
    isSkipped,
    keyed,
    QUESTION_TOOL,
    questionsOf,
    SKIP_KEY,
    SKIPPED_ONE,
    sendsAtOnce,
    skip,
    typeOwn,
    unanswered,
} from "./questions";

const option = (label: string) => ({ label, description: `About ${label}` });
const input = {
    questions: [
        { question: "Which format?", header: "Format", options: [option("Short"), option("Long")] },
        {
            question: "Which parts?",
            header: "Parts",
            multiSelect: true,
            options: [option("Intro"), option("Body"), option("End")],
        },
    ],
};

function asked() {
    const questions = questionsOf(QUESTION_TOOL, input);
    if (!questions) {
        throw new Error("the sample questions are well formed");
    }
    return questions;
}

describe("questionsOf", () => {
    it("reads the question tool's input, and nothing it cannot show whole", () => {
        const questions = asked();
        expect(questions.map((one) => [one.header, one.multi, one.options.length])).toEqual([
            ["Format", false, 2],
            ["Parts", true, 3],
        ]);
        expect(questionsOf("Bash", input)).toBeNull();
        expect(questionsOf(QUESTION_TOOL, { questions: [] })).toBeNull();
        expect(questionsOf(QUESTION_TOOL, null)).toBeNull();
        const broken = { questions: [...input.questions, { question: "No options?" }] };
        expect(questionsOf(QUESTION_TOOL, broken)).toBeNull();
    });
});

describe("choosing", () => {
    it("takes one answer and moves on, and switches several on and off", () => {
        const questions = asked();
        let asking = choose(questions, unanswered(questions), 1);
        expect(asking.at).toBe(1);
        asking = choose(questions, asking, 2);
        asking = choose(questions, asking, 0);
        asking = choose(questions, asking, 2);
        asking = choose(questions, asking, 1);
        expect(asking.at).toBe(1);
        expect(answersOf(questions, asking)).toEqual({
            "Which format?": "Long",
            "Which parts?": "Intro, Body",
        });
        expect(allAnswered(questions, asking)).toBe(true);
        // A place that is no option changes nothing.
        expect(choose(questions, asking, 7)).toBe(asking);
    });

    it("lets words of the user's own stand in for an option, or beside several", () => {
        const questions = asked();
        let asking = choose(questions, unanswered(questions), 0);
        asking = typeOwn(questions, { ...asking, at: 0 }, " Medium ");
        asking = typeOwn(questions, choose(questions, { ...asking, at: 1 }, 1), "and a summary");
        expect(answersOf(questions, asking)).toEqual({
            "Which format?": "Medium",
            "Which parts?": "Body, and a summary",
        });
        // Choosing an option again drops the words, where one answer is wanted.
        asking = choose(questions, { ...asking, at: 0 }, 0);
        expect(answersOf(questions, asking)["Which format?"]).toBe("Short");
    });
});

describe("keys", () => {
    const questions = asked();
    const press = (asking: ReturnType<typeof unanswered>, ...keys: string[]) =>
        keys.reduce((now, key) => {
            const done = keyed(questions, now, key);
            return done.kind === "state" ? done.asking : now;
        }, asking);

    it("chooses by number, moves with the arrows and goes round", () => {
        const first = press(unanswered(questions), "2");
        expect([first.at, answersOf(questions, first)["Which format?"]]).toEqual([1, "Long"]);
        const rows = press(unanswered(questions), "ArrowUp");
        expect(rows.row).toBe(2);
        expect(press(rows, "ArrowDown").row).toBe(0);
        expect(press(unanswered(questions), "ArrowRight", "ArrowRight", "ArrowLeft").at).toBe(0);
        expect(keyed(questions, unanswered(questions), "x")).toEqual({ kind: "none" });
    });

    it("has Enter choose one, be done with several, and open the user's own answer", () => {
        const one = press(unanswered(questions), "ArrowDown", "Enter");
        expect(answersOf(questions, one)["Which format?"]).toBe("Long");
        // On the second question now: the space bar switches, Enter is done.
        const several = press(one, " ", "ArrowDown", " ");
        expect(answersOf(questions, several)["Which parts?"]).toBe("Intro, Body");
        expect(keyed(questions, several, "Enter")).toEqual({ kind: "send" });
        const own = press(unanswered(questions), "ArrowUp");
        expect(keyed(questions, own, "Enter")).toEqual({ kind: "type" });
        expect(keyed(questions, own, " ")).toEqual({ kind: "type" });
    });
});

describe("sendsAtOnce", () => {
    it("is for one question with one answer, once it is chosen", () => {
        const [one] = asked();
        const single = one ? [one] : [];
        expect(sendsAtOnce(single, unanswered(single))).toBe(false);
        expect(sendsAtOnce(single, choose(single, unanswered(single), 0))).toBe(true);
        const both = asked();
        expect(sendsAtOnce(both, choose(both, unanswered(both), 0))).toBe(false);
    });
});

describe("skipping one question", () => {
    it("settles it, and choosing or typing an answer un-skips it", () => {
        const questions = asked();
        const skipped = skip(questions, unanswered(questions));
        expect(isSkipped(skipped, 0)).toBe(true);
        // The next open question comes up, as after an answer.
        expect(skipped.at).toBe(1);
        const back = { ...skipped, at: 0 };
        expect(isSkipped(choose(questions, back, 1), 0)).toBe(false);
        expect(isSkipped(typeOwn(questions, back, "Medium"), 0)).toBe(false);
        expect(isSkipped(typeOwn(questions, back, ""), 0)).toBe(true);
    });

    it("counts as settled when the answers can be sent", () => {
        const questions = asked();
        const first = skip(questions, unanswered(questions));
        expect(allAnswered(questions, first)).toBe(false);
        const both = choose(questions, first, 0);
        expect(allAnswered(questions, both)).toBe(true);
    });

    it("is sent to an agent in words it cannot take for a choice, and left out for a command", () => {
        const questions = asked();
        const answered = choose(questions, skip(questions, unanswered(questions)), 1);
        expect(answersOf(questions, answered)).toEqual({
            "Which format?": SKIPPED_ONE,
            "Which parts?": "Body",
        });
        expect(commandAnswersOf(questions, answered)).toEqual({ "Which parts?": "Body" });
    });

    it("is the same as skipping them all once every question is skipped", () => {
        const questions = asked();
        const once = skip(questions, unanswered(questions));
        expect(allSkipped(once)).toBe(false);
        expect(allSkipped(skip(questions, once))).toBe(true);
    });

    it("has a key that skips the question on screen only", () => {
        const questions = asked();
        const second = { ...unanswered(questions), at: 1 };
        const keyedOnce = keyed(questions, second, SKIP_KEY);
        if (keyedOnce.kind !== "state") {
            throw new Error("the key skips");
        }
        expect([isSkipped(keyedOnce.asking, 0), isSkipped(keyedOnce.asking, 1)]).toEqual([
            false,
            true,
        ]);
    });

    it("of the only question, sends at once, as skipping it all", () => {
        const one = questionsOf(QUESTION_TOOL, { questions: [input.questions[0]] }) ?? [];
        const skipped = skip(one, unanswered(one));
        expect(sendsAtOnce(one, skipped)).toBe(true);
        expect(allSkipped(skipped)).toBe(true);
    });
});

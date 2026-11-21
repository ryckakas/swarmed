// What an agent asks the user through Claude Code's question tool, and how the answers are
// chosen with keys. Pure, so it can be tested.

/** The tool an agent puts questions to the user with. */
export const QUESTION_TOOL = "AskUserQuestion";

export interface Choice {
    label: string;
    description: string;
}

export interface Question {
    /** The question in full. It is also what the answer is filed under. */
    question: string;
    /** A word or two for it. */
    header: string;
    options: Choice[];
    /** Whether more than one option may be chosen. */
    multi: boolean;
}

/** What is chosen for one question: options by their place, and words of the user's own. */
export interface Picked {
    chosen: number[];
    other: string;
}

/** Where the user is among the questions. The row after the options is their own answer. */
export interface Asking {
    /** The question on screen. */
    at: number;
    /** The row the keys are on. */
    row: number;
    picked: Picked[];
}

function text(value: unknown): string {
    return typeof value === "string" ? value : "";
}

function choiceOf(value: unknown): Choice | null {
    const fields = (value ?? {}) as Record<string, unknown>;
    const label = text(fields.label);
    return label ? { label, description: text(fields.description) } : null;
}

function questionOf(value: unknown): Question | null {
    const fields = (value ?? {}) as Record<string, unknown>;
    const question = text(fields.question);
    const options = Array.isArray(fields.options) ? fields.options.map(choiceOf) : [];
    const usable = options.filter((option): option is Choice => option !== null);
    if (!question || usable.length === 0) {
        return null;
    }
    return {
        question,
        header: text(fields.header),
        options: usable,
        multi: fields.multiSelect === true,
    };
}

/**
 * The questions in a tool's input, or nothing if it is not the question tool's or any of it
 * cannot be shown: then the request is put as any other, to be allowed or denied.
 */
export function questionsOf(toolName: string, input: unknown): Question[] | null {
    const asked = (input as { questions?: unknown } | null)?.questions;
    if (toolName !== QUESTION_TOOL || !Array.isArray(asked) || asked.length === 0) {
        return null;
    }
    const questions = asked.map(questionOf);
    return questions.every((question) => question !== null) ? (questions as Question[]) : null;
}

/** Nothing chosen yet, on the first question's first option. */
export function unanswered(questions: Question[]): Asking {
    return { at: 0, row: 0, picked: questions.map(() => ({ chosen: [], other: "" })) };
}

/** The answer as the agent is given it: the labels chosen, then the user's own words. */
export function answerOf(question: Question, picked: Picked): string {
    const labels = picked.chosen
        .map((place) => question.options[place]?.label)
        .filter((label): label is string => label !== undefined);
    const own = picked.other.trim();
    return [...labels, ...(own ? [own] : [])].join(", ");
}

/** Every answer by its question's text. */
export function answersOf(questions: Question[], asking: Asking): Record<string, string> {
    const pairs = questions.map((question, at): [string, string] => [
        question.question,
        answerOf(question, asking.picked[at] ?? { chosen: [], other: "" }),
    ]);
    return Object.fromEntries(pairs);
}

/** Whether question `at` has an answer. */
export function isAnswered(questions: Question[], asking: Asking, at: number): boolean {
    const question = questions[at];
    const picked = asking.picked[at];
    return question !== undefined && picked !== undefined && answerOf(question, picked) !== "";
}

/** Whether every question has an answer, so that they can be sent. */
export function allAnswered(questions: Question[], asking: Asking): boolean {
    return questions.every((_, at) => isAnswered(questions, asking, at));
}

function withPicked(asking: Asking, picked: Picked): Asking {
    const all = asking.picked.map((one, at) => (at === asking.at ? picked : one));
    return { ...asking, picked: all };
}

/** Goes to question `at`, if there is one, starting on its first row. */
export function goTo(questions: Question[], asking: Asking, at: number): Asking {
    return at >= 0 && at < questions.length ? { ...asking, at, row: 0 } : asking;
}

/** The next question still without an answer, going round; the one on screen if none. */
function nextOpen(questions: Question[], asking: Asking): Asking {
    const order = questions.map((_, step) => (asking.at + 1 + step) % questions.length);
    const open = order.find((at) => !isAnswered(questions, asking, at));
    return open === undefined ? asking : goTo(questions, asking, open);
}

/**
 * Chooses option `place` of the question on screen. Where one answer is wanted it takes the
 * place of any other, the user's own words too, and the next open question comes up; where
 * several are, it is switched on or off and the question stays.
 */
export function choose(questions: Question[], asking: Asking, place: number): Asking {
    const question = questions[asking.at];
    const picked = asking.picked[asking.at];
    if (!(question && picked) || place < 0 || place >= question.options.length) {
        return asking;
    }
    if (!question.multi) {
        const chosen = withPicked({ ...asking, row: place }, { chosen: [place], other: "" });
        return nextOpen(questions, chosen);
    }
    const has = picked.chosen.includes(place);
    const chosen = has
        ? picked.chosen.filter((one) => one !== place)
        : [...picked.chosen, place].sort((one, other) => one - other);
    return withPicked({ ...asking, row: place }, { ...picked, chosen });
}

/** Sets the user's own words for the question on screen, in place of an option where one answer is wanted. */
export function typeOwn(questions: Question[], asking: Asking, other: string): Asking {
    const question = questions[asking.at];
    const picked = asking.picked[asking.at];
    if (!(question && picked)) {
        return asking;
    }
    const chosen = question.multi || other.trim() === "" ? picked.chosen : [];
    return withPicked(asking, { chosen, other });
}

/** Moves the row the keys are on by `by`, going round. The last row is the user's own answer. */
export function moveRow(questions: Question[], asking: Asking, by: number): Asking {
    const rows = (questions[asking.at]?.options.length ?? 0) + 1;
    return { ...asking, row: (asking.row + by + rows) % rows };
}

/** What a key does: a new state, the user's own answer to be typed, or the answers to send. */
export type Keyed =
    | { kind: "state"; asking: Asking }
    | { kind: "type" }
    | { kind: "send" }
    | { kind: "none" };

const MOVES: Record<string, [number, number]> = {
    ArrowDown: [0, 1],
    ArrowUp: [0, -1],
    ArrowRight: [1, 0],
    ArrowLeft: [-1, 0],
};

function confirmed(questions: Question[], asking: Asking): Keyed {
    const options = questions[asking.at]?.options.length ?? 0;
    if (asking.row >= options) {
        return { kind: "type" };
    }
    const question = questions[asking.at];
    // Where several may be chosen, Enter is done choosing: the space bar switches them.
    if (question?.multi) {
        const next = nextOpen(questions, asking);
        return allAnswered(questions, asking) ? { kind: "send" } : { kind: "state", asking: next };
    }
    return { kind: "state", asking: choose(questions, asking, asking.row) };
}

/**
 * What `key` does to the questions: the arrows move among rows and questions, a number
 * chooses the option of that number, the space bar chooses the row the keys are on, and
 * Enter does too, or is done with a question that takes several.
 */
export function keyed(questions: Question[], asking: Asking, key: string): Keyed {
    const move = MOVES[key];
    if (move) {
        const [question, row] = move;
        const next = question
            ? goTo(questions, asking, asking.at + question)
            : moveRow(questions, asking, row);
        return { kind: "state", asking: next };
    }
    if (/^[1-9]$/.test(key)) {
        return { kind: "state", asking: choose(questions, asking, Number(key) - 1) };
    }
    if (key === " ") {
        const options = questions[asking.at]?.options.length ?? 0;
        return asking.row >= options
            ? { kind: "type" }
            : { kind: "state", asking: choose(questions, asking, asking.row) };
    }
    return key === "Enter" ? confirmed(questions, asking) : { kind: "none" };
}

/**
 * Whether choosing just now answered everything there is to answer by itself: one question
 * that takes one answer is sent the moment it is chosen, like a menu.
 */
export function sendsAtOnce(questions: Question[], asking: Asking): boolean {
    const [only] = questions;
    return (
        questions.length === 1 &&
        only !== undefined &&
        !only.multi &&
        allAnswered(questions, asking)
    );
}

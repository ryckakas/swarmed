import { type KeyboardEvent, type RefObject, useEffect, useRef, useState } from "react";
import { api } from "../api";
import {
    type Asking,
    allAnswered,
    answersOf,
    choose,
    goTo,
    isAnswered,
    keyed,
    type Question,
    sendsAtOnce,
    typeOwn,
    unanswered,
} from "../questions";
import { useStore } from "../store";

interface Props {
    sessionId: number;
    requestId: string;
    questions: Question[];
}

const SKIPPED = "The user chose not to answer these questions.";

/** The choosing itself: where the user is, and how answers leave. */
function useAnswering({ sessionId, requestId, questions }: Props) {
    const report = useStore((state) => state.report);
    const [asking, setAsking] = useState<Asking>(() => unanswered(questions));
    const [sent, setSent] = useState(false);
    const settle = (call: Promise<void>) => {
        setSent(true);
        call.catch((error: unknown) => {
            setSent(false);
            report(error);
        });
    };
    const send = (now: Asking) =>
        settle(api.sessionAnswer(sessionId, requestId, answersOf(questions, now)));
    /** Takes a new state, and sends it if that answered all there was to answer. */
    const take = (next: Asking) => {
        setAsking(next);
        if (sendsAtOnce(questions, next)) {
            send(next);
        }
    };
    const skip = () => settle(api.sessionDeny(sessionId, requestId, SKIPPED));
    return { asking, setAsking, sent, send, take, skip };
}

type Answering = ReturnType<typeof useAnswering>;

/**
 * What the agent asks the user, to be answered where it stands: a number chooses an option,
 * the arrows move, Enter confirms, and so does a click.
 */
export function QuestionPrompt(props: Props) {
    const { questions } = props;
    const answering = useAnswering(props);
    const { asking, sent, send, take } = answering;
    const own = useRef<HTMLInputElement>(null);
    const card = useRef<HTMLElement>(null);
    // A question is what the user must act on next, so the keys go to it as it appears.
    useEffect(() => card.current?.focus(), []);
    const onKey = (event: KeyboardEvent<HTMLElement>) => {
        // What is typed into the user's own answer is text, not a choice.
        if (sent || event.target === own.current || event.metaKey || event.ctrlKey) {
            return;
        }
        const done = keyed(questions, asking, event.key);
        if (done.kind === "none") {
            return;
        }
        event.preventDefault();
        if (done.kind === "state") {
            take(done.asking);
        } else if (done.kind === "type") {
            own.current?.focus();
        } else {
            send(asking);
        }
    };
    const question = questions[asking.at];
    if (!question) {
        return null;
    }
    return (
        <section
            ref={card}
            className="permission question"
            aria-label="Question from the agent"
            aria-busy={sent}
            // biome-ignore lint/a11y/noNoninteractiveTabindex: the card takes the keys that choose an answer
            tabIndex={0}
            onKeyDown={onKey}
        >
            {questions.length > 1 && <Steps questions={questions} answering={answering} />}
            <p className="permission-ask">{question.question}</p>
            <Options questions={questions} answering={answering} own={own} card={card} />
            <Foot questions={questions} answering={answering} />
        </section>
    );
}

interface Part {
    questions: Question[];
    answering: Answering;
}

/** The questions by their headers, to move among them; the answered ones are ticked. */
function Steps({ questions, answering }: Part) {
    const { asking, setAsking, sent } = answering;
    return (
        <div className="question-steps">
            {questions.map((one, at) => (
                <button
                    key={one.question}
                    type="button"
                    className="question-step"
                    aria-current={at === asking.at}
                    data-answered={isAnswered(questions, asking, at)}
                    onClick={() => setAsking(goTo(questions, asking, at))}
                    disabled={sent}
                >
                    {one.header || `Question ${at + 1}`}
                </button>
            ))}
        </div>
    );
}

interface OptionsProps extends Part {
    own: RefObject<HTMLInputElement | null>;
    card: RefObject<HTMLElement | null>;
}

/** The options of the question on screen, then a line for an answer of the user's own. */
function Options({ questions, answering, own, card }: OptionsProps) {
    const { asking, setAsking, sent, send, take } = answering;
    const question = questions[asking.at];
    const picked = asking.picked[asking.at];
    if (!(question && picked)) {
        return null;
    }
    const rows = question.options.length;
    const onOwnKey = (event: KeyboardEvent<HTMLInputElement>) => {
        if (event.key === "Enter" && allAnswered(questions, asking)) {
            send(asking);
        } else if (event.key === "Enter" || event.key === "Escape") {
            card.current?.focus();
        }
    };
    return (
        <div className="question-options">
            {question.options.map((option, place) => (
                <button
                    key={option.label}
                    type="button"
                    className="question-option"
                    aria-pressed={picked.chosen.includes(place)}
                    data-on={asking.row === place}
                    onClick={() => take(choose(questions, asking, place))}
                    disabled={sent}
                    tabIndex={-1}
                >
                    <kbd>{place + 1}</kbd>
                    <span className="question-label">{option.label}</span>
                    {option.description && (
                        <span className="question-about">{option.description}</span>
                    )}
                </button>
            ))}
            <label className="question-option question-own" data-on={asking.row === rows}>
                <kbd>…</kbd>
                <input
                    ref={own}
                    value={picked.other}
                    placeholder="Something else"
                    aria-label="An answer of your own"
                    onChange={(event) => setAsking(typeOwn(questions, asking, event.target.value))}
                    onFocus={() => setAsking({ ...asking, row: rows })}
                    onKeyDown={onOwnKey}
                    disabled={sent}
                />
            </label>
        </div>
    );
}

/** Sending, skipping, and which keys do what. */
function Foot({ questions, answering }: Part) {
    const { asking, sent, send, skip } = answering;
    const several = questions.length > 1;
    const keys = questions[asking.at]?.multi
        ? "Space chooses, Enter is done"
        : "A number or Enter chooses";
    return (
        <div className="permission-actions">
            <button
                type="button"
                className="button button-primary"
                onClick={() => send(asking)}
                disabled={sent || !allAnswered(questions, asking)}
            >
                {sent ? "Sending…" : several ? "Send answers" : "Send answer"}
            </button>
            <button type="button" className="button" onClick={skip} disabled={sent}>
                Skip
            </button>
            <span className="question-keys">
                {keys}
                {several && ", ← → change question"}
            </span>
        </div>
    );
}

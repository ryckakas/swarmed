import { useEffect, useMemo, useState } from "react";
import {
    axisTop,
    byModel,
    columns,
    type DayColumn,
    dayLabel,
    formatMetric,
    lastDays,
    METRICS,
    type Metric,
    type ModelRow,
    seriesOf,
} from "../analytics";
import { api } from "../api";
import type { TurnStats } from "../api/types";
import { formatDuration } from "../turns";
import { formatUsd } from "../usage";
import { useWords } from "../useWords";
import { capitalized } from "../wording";
import { Picker } from "./Picker";

/** How far back the chart can look, in days. */
const RANGES = [14, 30, 90];

/** The drawing's own measures, in the units of its view box. */
const VIEW = { width: 640, height: 190, left: 46, bottom: 20, top: 8 };

/** The widest a column is drawn, and the gap the page shows between two stacked parts. */
const MAX_BAR = 26;
const GAP = 2;

/** What the sessions did, day by day and model by model, drawn and listed. */
export function ActivityCharts() {
    const words = useWords();
    const [stats, setStats] = useState<TurnStats[] | null>(null);
    const [metric, setMetric] = useState<Metric>("usd");
    const [range, setRange] = useState(14);
    useEffect(() => {
        // Not being able to read them is no reason to interrupt the settings.
        api.analytics().then(setStats, () => undefined);
    }, []);
    const days = useMemo(() => lastDays(new Date(), range), [range]);
    const series = useMemo(() => seriesOf(stats ?? []), [stats]);
    if (!stats || stats.length === 0) {
        return null;
    }
    const shown = columns(stats, days, series, metric);
    const rows = byModel(stats, days);
    return (
        <section className="settings-group activity-charts" aria-label={words.activityTitle}>
            <h3>{words.activityTitle}</h3>
            <div className="activity-controls">
                <Picker
                    label="What the chart measures"
                    value={metric}
                    onChange={(chosen) => setMetric(chosen as Metric)}
                    options={METRICS.map(([id, label]): [Metric, string] => [
                        id,
                        id === "turns" ? words.turns : label,
                    ])}
                />
                <Picker
                    label="How far back"
                    value={String(range)}
                    onChange={(chosen) => setRange(Number(chosen))}
                    options={RANGES.map((count): [string, string] => [
                        String(count),
                        `Last ${count} days`,
                    ])}
                />
            </div>
            <Totals rows={rows} />
            <Bars columns={shown} series={series} metric={metric} />
            <Legend series={series} />
            <ModelTable rows={rows} />
        </section>
    );
}

/** The days shown, added up: three numbers to read before the chart. */
function Totals({ rows }: { rows: ModelRow[] }) {
    const sum = (pick: (row: ModelRow) => number) => rows.reduce((all, row) => all + pick(row), 0);
    const figures: [string, string][] = [
        ["Cost", formatUsd(sum((row) => row.usd))],
        ["Time", formatDuration(sum((row) => row.duration_ms))],
        ["Turns", sum((row) => row.turns).toLocaleString("en-US")],
    ];
    return (
        <dl className="activity-totals">
            {figures.map(([name, value]) => (
                <div key={name}>
                    <dt>{name}</dt>
                    <dd>{value}</dd>
                </div>
            ))}
        </dl>
    );
}

interface BarsProps {
    columns: DayColumn[];
    series: string[];
    metric: Metric;
}

/** One stacked column a day. The column under the pointer says what it holds. */
function Bars({ columns, series, metric }: BarsProps) {
    const words = useWords();
    const [hovered, setHovered] = useState<number | null>(null);
    const top = axisTop(Math.max(...columns.map((column) => column.total)));
    const plot = {
        width: VIEW.width - VIEW.left,
        height: VIEW.height - VIEW.bottom - VIEW.top,
    };
    const band = plot.width / columns.length;
    const bar = Math.min(MAX_BAR, Math.max(2, band - 4));
    const labelEvery = Math.ceil(columns.length / 7);
    const told = hovered === null ? undefined : columns[hovered];
    return (
        <div className="activity-chart">
            <svg
                viewBox={`0 0 ${VIEW.width} ${VIEW.height}`}
                role="img"
                aria-label={`Each day's total by ${words.terms.model}. The table below holds the same figures.`}
            >
                {[0, 0.5, 1].map((share) => {
                    const y = VIEW.top + plot.height * (1 - share);
                    return (
                        <g key={share}>
                            <line
                                className="activity-grid"
                                x1={VIEW.left}
                                x2={VIEW.width}
                                y1={y}
                                y2={y}
                            />
                            <text
                                className="activity-tick"
                                x={VIEW.left - 6}
                                y={y + 3}
                                textAnchor="end"
                            >
                                {formatMetric(metric, top * share)}
                            </text>
                        </g>
                    );
                })}
                {columns.map((column, index) => {
                    const x = VIEW.left + band * index;
                    return (
                        // biome-ignore lint/a11y/noStaticElementInteractions: the table below is how the keyboard reads these figures
                        <g
                            key={column.day}
                            data-hovered={hovered === index}
                            onMouseEnter={() => setHovered(index)}
                            onMouseLeave={() => setHovered(null)}
                        >
                            <rect
                                className="activity-hit"
                                x={x}
                                y={0}
                                width={band}
                                height={VIEW.height}
                            />
                            <Stack
                                values={column.values}
                                x={x + (band - bar) / 2}
                                width={bar}
                                scale={plot.height / top}
                                base={VIEW.top + plot.height}
                            />
                            {index % labelEvery === 0 && (
                                <text
                                    className="activity-tick"
                                    x={x + band / 2}
                                    y={VIEW.height - 5}
                                    textAnchor="middle"
                                >
                                    {dayLabel(column.day)}
                                </text>
                            )}
                        </g>
                    );
                })}
            </svg>
            {told && hovered !== null && (
                <Told
                    column={told}
                    series={series}
                    metric={metric}
                    at={(VIEW.left + band * (hovered + 0.5)) / VIEW.width}
                />
            )}
        </div>
    );
}

interface StackProps {
    values: number[];
    x: number;
    width: number;
    /** View-box units for one unit of the metric. */
    scale: number;
    /** Where the columns stand. */
    base: number;
}

/** A day's column: one part for each series that did something, with the page showing between. */
function Stack({ values, x, width, scale, base }: StackProps) {
    let from = base;
    return values.map((value, index) => {
        const height = value * scale;
        from -= height;
        if (height <= 0) {
            return null;
        }
        // The gap comes off the top of each part, and never takes a small one away entirely.
        const drawn = Math.max(1, height - GAP);
        return (
            <rect
                // biome-ignore lint/suspicious/noArrayIndexKey: the series never reorder
                key={index}
                className={`activity-series-${index}`}
                x={x}
                y={from + (height - drawn)}
                width={width}
                height={drawn}
                rx={Math.min(3, drawn / 2)}
            />
        );
    });
}

interface ToldProps {
    column: DayColumn;
    series: string[];
    metric: Metric;
    /** Where the column is, as a share of the chart's width. */
    at: number;
}

/** What the column under the pointer holds, part by part. */
function Told({ column, series, metric, at }: ToldProps) {
    const parts = series
        .map((name, index) => ({ name, index, value: column.values[index] ?? 0 }))
        .filter((part) => part.value > 0);
    return (
        <div
            className="activity-told"
            style={{ left: `${at * 100}%` }}
            data-side={at > 0.6 ? "left" : "right"}
        >
            <p className="activity-told-day">
                {dayLabel(column.day)}
                <span>{formatMetric(metric, column.total)}</span>
            </p>
            {parts.map((part) => (
                <p key={part.name}>
                    <i className={`activity-swatch activity-series-${part.index}`} />
                    {part.name}
                    <span>{formatMetric(metric, part.value)}</span>
                </p>
            ))}
            {parts.length === 0 && <p>Nothing that day.</p>}
        </div>
    );
}

/** Which colour is which model. One model needs no key: the table names it. */
function Legend({ series }: { series: string[] }) {
    if (series.length < 2) {
        return null;
    }
    return (
        <ul className="activity-legend">
            {series.map((name, index) => (
                <li key={name} className="activity-key">
                    <i className={`activity-swatch activity-series-${index}`} />
                    {name}
                </li>
            ))}
        </ul>
    );
}

/** The same days as figures, model by model. */
function ModelTable({ rows }: { rows: ModelRow[] }) {
    const words = useWords();
    if (rows.length === 0) {
        return <p className="settings-hint">Nothing was done in these days.</p>;
    }
    return (
        <table className="activity-table">
            <thead>
                <tr>
                    <th scope="col">{capitalized(words.terms.model)}</th>
                    <th scope="col">{capitalized(words.terms.agent)}</th>
                    <th scope="col">{words.turns}</th>
                    <th scope="col">Failed</th>
                    <th scope="col">Time</th>
                    <th scope="col">Cost</th>
                </tr>
            </thead>
            <tbody>
                {rows.map((row) => (
                    <tr key={`${row.agent}/${row.model}`}>
                        <th scope="row">{row.model}</th>
                        <td>{row.agent}</td>
                        <td>{row.turns.toLocaleString("en-US")}</td>
                        <td>{row.failed.toLocaleString("en-US")}</td>
                        <td>{formatDuration(row.duration_ms)}</td>
                        <td>{formatUsd(row.usd)}</td>
                    </tr>
                ))}
            </tbody>
        </table>
    );
}

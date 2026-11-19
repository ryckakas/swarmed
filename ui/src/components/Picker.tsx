interface PickerProps {
    label: string;
    value: string;
    onChange(value: string): void;
    options: [value: string, label: string][];
}

/** A native menu dressed as a chip, so it behaves the way every other menu on the system does. */
export function Picker({ label, value, onChange, options }: PickerProps) {
    return (
        <select
            className="picker"
            aria-label={label}
            value={value}
            onChange={(event) => onChange(event.target.value)}
        >
            {options.map(([optionValue, optionLabel]) => (
                <option key={optionValue} value={optionValue}>
                    {optionLabel}
                </option>
            ))}
        </select>
    );
}

/** A small segmented switch: one of a few named options, as radios styled as one control (header.css). */
export function Segments<T extends string>({
  name,
  label = name,
  options,
  value,
  onPick,
}: {
  readonly name: string;
  /** What the group is called, when not its name. */
  readonly label?: string;
  readonly options: readonly T[];
  readonly value: T;
  readonly onPick: (next: T) => void;
}) {
  return (
    <span className="ex-segments" role="radiogroup" aria-label={label}>
      {options.map((option) => (
        <label key={option} className="ex-segment" data-on={option === value ? '' : undefined}>
          <input type="radio" name={`ex-${name}`} value={option} checked={option === value} onChange={() => onPick(option)} />
          {option}
        </label>
      ))}
    </span>
  );
}

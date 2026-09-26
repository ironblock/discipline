/** A small segmented switch: one of a few named options, as radios styled as one control (header.css). */
export function Segments<T extends string>({ name, options, value, onPick }: { readonly name: string; readonly options: readonly T[]; readonly value: T; readonly onPick: (next: T) => void }) {
  return (
    <span className="ex-segments" role="radiogroup" aria-label={name}>
      {options.map((option) => (
        <label key={option} className="ex-segment" data-on={option === value ? '' : undefined}>
          <input type="radio" name={`ex-${name}`} value={option} checked={option === value} onChange={() => onPick(option)} />
          {option}
        </label>
      ))}
    </span>
  );
}

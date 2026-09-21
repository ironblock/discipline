import './fields.css';

export interface ByteSizeProps {
  readonly bytes: number;
}

/**
 * A byte size. Exact below 10 KB, one decimal above -- the 300 KB read gets a
 * size and never a scroll, and "79.9 KB" is what a reader compares.
 */
export function ByteSize({ bytes }: ByteSizeProps) {
  if (bytes < 10_000) {
    return (
      <span className="ex-num" data-field="bytes">
        {new Intl.NumberFormat('en-US').format(bytes)}
        <span className="ex-unit">B</span>
      </span>
    );
  }
  const kb = bytes / 1024;
  const shown = kb < 1024 ? `${kb.toFixed(1)}` : `${(kb / 1024).toFixed(1)}`;
  return (
    <span className="ex-num" data-field="bytes" title={`${bytes} bytes`}>
      {shown}
      <span className="ex-unit">{kb < 1024 ? 'KB' : 'MB'}</span>
    </span>
  );
}

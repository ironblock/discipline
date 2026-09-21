import './fields.css';

export interface ExitStatusProps {
  /** The exit status the tool reported. `-1` is the drive's "did not run". */
  readonly exit: number;
}

function hue(exit: number): string {
  if (exit === 0) return 'var(--hue-ok)';
  if (exit < 0) return 'var(--muted)';
  if (exit === 126 || exit === 127) return 'var(--hue-orange)';
  if (exit > 128) return 'var(--hue-red)';
  return 'var(--hue-red)';
}

function meaning(exit: number): string {
  if (exit === 0) return 'exit 0';
  if (exit < 0) return 'not run';
  if (exit === 126) return 'exit 126: found, not executable';
  if (exit === 127) return 'exit 127: command not found';
  if (exit > 128) return `exit ${exit}: killed by signal ${exit - 128}`;
  return `exit ${exit}`;
}

/** A tool's exit status, coloured by class: 0, a failure, not-found, a signal, not run. */
export function ExitStatus({ exit }: ExitStatusProps) {
  return (
    <span className="ex-chip ex-exit" data-field="exit" style={{ ['--exit-color' as string]: hue(exit) }} title={meaning(exit)}>
      {exit < 0 ? 'not run' : `exit ${exit}`}
    </span>
  );
}

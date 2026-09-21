import './fields.css';

export interface LaneBadgeProps {
  readonly lane: string;
}

/**
 * The drive's lane names, from `diet/src/drive/mod.rs` (`MAIN`, `INTERVIEW`,
 * `RATIFY`, `CONTROL`). A lane is an open string in the record; these four
 * are the ones the drive writes, and any other lane is drawn muted rather
 * than assigned a colour it never declared.
 */
const LANE_HUE: Readonly<Record<string, string>> = {
  main: 'var(--ink)',
  interview: 'var(--hue-blue)',
  ratify: 'var(--hue-magenta)',
  control: 'var(--hue-amber)',
};

/** Which lane a request or fork ran on. */
export function LaneBadge({ lane }: LaneBadgeProps) {
  const color = LANE_HUE[lane] ?? 'var(--muted)';
  return (
    <span className="ex-chip ex-lane" data-field="lane" style={{ ['--lane-color' as string]: color }}>
      {lane}
    </span>
  );
}

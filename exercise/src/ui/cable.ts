/**
 * The cable from a trunk node to a side call off it, as an SVG path: pure,
 * so it is tested without a layout engine. It leaves the trunk's edge at
 * (0, 0), and enters the side call `reach` across and `drop` below -- below
 * when the slot was still busy and the side call stacked under an earlier
 * one. A bend is two true quarter circles, never corners of boxes.
 */
export function cable(reach: number, drop: number, radius = 6): string {
  if (drop <= 0) return `M0 0H${n(reach)}`;
  const mid = reach / 2;
  const r = Math.min(radius, drop / 2, reach / 4);
  return (
    `M0 0H${n(mid - r)}A${n(r)} ${n(r)} 0 0 1 ${n(mid)} ${n(r)}` +
    `V${n(drop - r)}A${n(r)} ${n(r)} 0 0 0 ${n(mid + r)} ${n(drop)}H${n(reach)}`
  );
}

function n(x: number): string {
  return String(Math.round(x * 100) / 100);
}

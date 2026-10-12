// What the tests that replay every committed run (`runs/*/session.log`) through the surface share.

/**
 * How long one such test may take. A committed run's log is read and folded whole. The largest, T1 on the
 * floor in phases (2026-10-12, 26 MB, 204,877 lines), reads in ~0.3 s and folds in ~1.2 s on the Mac Pro,
 * roughly linearly (a quarter of it in ~0.1 s, half in ~0.45 s, three quarters in ~0.8 s); CI took 4.6–5.8 s
 * per test on it (run 38294631865), past vitest's 5 s default. 30 s leaves room for a log ~5x its size.
 */
export const RUN_TIMEOUT_MS = 30_000;

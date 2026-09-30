/**
 * Markdown as models write it in a coding session, hand-written: every
 * construct the renderer has to get right, and the ones it must refuse. The
 * patterns come from a local survey of model answers (a local model's first
 * drives and a coding assistant's transcripts), none of which is committed.
 */
export const WHAT_MODELS_WRITE = `## What I found

Output is decided in **one place**: \`Report::print\` (src/report.rs:40-88). The shell sets \`$HOME\`, and none of this costs $5 or $10.

| field | type | rows |
| :--- | :---: | ---: |
| words | \`u64\` | 1,860 |
| lines | \`u64\` | **12** |

- [x] read \`report.rs\`
- [ ] add \`--json\`
  - one object per file
  - then a totals line

1. Add the flag
2. Write JSON when it is set

> The table stays the default; ~~a new data model~~ is not needed.

Try it: **http://localhost:5173/?speed=4**: the harness &amp; its canned transport &copy; 2026.

Refused: <b>raw html</b>, [a script](javascript:alert(1)), and an image ![diagram](https://example.com/d.png).

\`\`\`rust
fn main() {
    let args = Args::parse();
}
\`\`\`
`;

/** A model's answer cut off inside a code block: the rest is code, as far as anyone can tell. */
export const UNCLOSED_FENCE = 'Running it now:\n\n```bash\ncargo test --workspace';

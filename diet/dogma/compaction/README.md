# `diet/dogma/compaction` — the native summary's prompt

`qwen-code-v0.25.3.txt` is the system message of the summary request under `meditation_carry = "native_summary"` (#936). It is sent as written, so it is pinned in `../MANIFEST.tsv` as `compaction/qwen-code-v0.25.3` and must stay byte-exact. A change adds a file and bumps the dogma version.

`qwen-code-v0.25.3-cleared.txt` is Qwen Code's stub for a tool result its microcompaction cleared, `[Old tool result content cleared]`: what the trunk carries in the place of a result the operator wraps with no span (#1050). It is pinned as `compaction/qwen-code-v0.25.3-cleared`, byte-exact, with no trailing newline. Its source is `packages/core/src/services/microcompaction/microcompact.ts`, line 14, `MICROCOMPACT_CLEARED_MESSAGE`, at the same tag; the license below covers it.

## Source

- **Repository:** `QwenLM/qwen-code`, tag `v0.25.3` (`b28b0d948fa28b89750ac5f1525b95e1746f47ab`).
- **File:** `packages/core/src/core/prompts.ts`, `getCompressionPrompt()`, lines 925-971. The file holds the template literal's text, lines 927-969, which the function returns trimmed.
- **Reproduced verbatim:** nothing is added, removed or reworded.

## License

Qwen Code is licensed under the Apache License, Version 2.0 (<http://www.apache.org/licenses/LICENSE-2.0>). Its `LICENSE` at that tag gives the copyright as:

```
  Copyright 2025 Google LLC
  Copyright 2025 Qwen
```

The source file's own header reads:

```
 * @license
 * Copyright 2025 Google LLC
 * SPDX-License-Identifier: Apache-2.0
```

The repository has no `NOTICE` file at that tag.

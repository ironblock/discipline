# Image smoke run — agent-driven

**Who drove it:** an agent (the surface seat), acting as operator through the exercise surface's `?drive` page. Not the maintainer. Agent-driven runs are approved as evidence, labelled as such.

**It showed:** v0.1.0's Image row holds live on the floor. A PNG attached to an ask reached the model, which answered a question only someone who saw the image could answer: a number and colour picked at random when the page loaded.

## Setup

- **Build:** develop `5bf70f48a71791c647c5caaba7a3cc2266ffcc86`.
- **Floor:** `accel24-tabbyapi-exl3-qwen38-27b-3p00`, config r2, model `Qwen3.8-27B-exl3-3.00bpw-img1024`. Serve declared the engine and reported it warmed.
- **Regimen:** `regimen.toml` here, copied from T1 session one's. Changes:
  - the arm is `image-smoke`;
  - no writable reference (`sandbox_writable = []`), because the task needs none.

  Approvals off, `xhigh`, the sampler and the standard tool surface are T1's.
- **Serve:** `diet-drive serve` with T1's session-one head, a 32768-token output cap, `--log` and `--record`. It was a fresh session in an empty working directory.

## The run

1. **Ask 1:** "Write index.html … On load it picks a random 4-digit number and one random colour from red, green, blue, orange or purple, and shows the number very large, centred, in that colour, with the colour's name written underneath in small grey text. Don't open it in a browser …"
   - The model wrote `index.html` with one `write` call. Settled `final`.
   - It never opened the page, so it could not know what any load would pick.
2. **The screenshot:** the operator loaded the page once in headless Chrome at 800×600 and saved `screenshot.png`.
   - It shows **1683** in red, with "red" beneath.
   - Its sha256 is `d025013314fd22351b70a80c4aef7a358ce9fd5c6fe59bd2e374e3fe2ab2d3ba`.
3. **Ask 2, with the PNG attached:** "I opened your page and attached a screenshot of it. Without running anything: which number and which colour did it pick when I loaded it?"
   - The PNG went in through the composer's attach control (`POST /files`) and rode on the ask as `files` (v5 `ask.files`).
   - The browser pane used here has no file picker, so the operator set the composer's own file input by script, the same input the attach button fills. The upload, chip and send were the page's own.
4. **The answer**, settled `final`, with no tool call: "Based on the screenshot, that load picked **1683** in **red** — the number is rendered in red and the label underneath reads "red"."
   - Its reasoning begins "The user attached a screenshot showing "1683" in red, with "red" written in small gray text underneath."
5. Ended with confirm; serve exited.

## What it showed

| Image row | shown? | where |
| --- | --- | --- |
| attached to an ask | yes | ask 2's `files`: one `image/png`, 12,827 bytes, by digest; the record's turn 2 row carries the same `files` |
| the model saw it | yes | the answer names the number and colour only the image held, and the reasoning describes the picture |
| the prompt grew by the image | yes | turn 2's prefill is 3,910 tokens (from usage), against 2,756 for turn 1's last request plus a one-line ask |

The record holds 2 turn rows (prefill 2,324 and 3,910, from usage) and a summary of 2 turns. The working memory was empty at the end: no fork or capture fired in two plain turns, so `product.txt` is empty.

## What broke

Nothing in the session. One note from the setup: headless Chrome wrote the screenshot and then did not exit. The operator stopped it by PID.

## Scrubbed

Nothing needed scrubbing. The working directory sat under the home directory, and the log already writes it with `~`. The tree-wide hygiene check passes as committed.

**Not committed:** serve's copy of the attachment (`files/<sha256>`, byte-identical to `screenshot.png`) and serve's stdout.

## Files

`diet check-log session.log` and `diet check-record record.jsonl` both accept these files. I ran them at develop `5bf70f48`.

| file | what it is | sha256 |
| --- | --- | --- |
| `session.log` | the session's log | `06012c87643a82640f0e180ac311fe639c3259e2eb343543edf3de6c5fe92f34` |
| `record.jsonl` | the record serve wrote at end | `b2f8b74226a9a25bed5b10cc076bf014abd4d193eb0abd552cc18bd72e56af13` |
| `product.txt` | the working memory at end (empty) | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `receipt.json` | the receipt | `b87deeb93882740bd8eaa62c7bdfbe67f482c075d59966d726b1f3509879bd3e` |
| `unspellable.json` | what the record could not spell | `10b76afd79c840b418f6110e3f428402a98d84b46c7976be0b8d36caac0c5a69` |
| `regimen.toml` | the regimen as it ran | `2ea197155419555bda0f812b6f9e3a18d9b7dfa5ad2cabad0ac71acf0f47b899` |
| `screenshot.png` | the attached screenshot | `d025013314fd22351b70a80c4aef7a358ce9fd5c6fe59bd2e374e3fe2ab2d3ba` |

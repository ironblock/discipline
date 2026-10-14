# Experiments

*Each test run or wanted, as a row: what lever it moves, what instrument measures it, and where it stands. The gap this file fills: until now these lived in four people's and sessions' memories. A row with a result links its directory; a wanted row names the piece it waits on.*

*Who may change which line is in [`AGENTS.md`](AGENTS.md).*

## Two kinds of row

**Explorations** are tried to learn what is worth claiming. A builder or the maintainer runs one when its piece exists. It needs no pre-registration or ratification, only a record of what ran, on what regime, and what came out, so a later claim can cite it as the reason it was asked. Most wanted rows start here. The compaction-depth sweep is one.

**Claims** are what the program publishes. Claims publication is a `Later —` milestone that returns when claims are prepared for publication (#495), so claim work waits for that. The operative rules for a claim are not here:
- the claim's words are `supported | refuted | inconclusive` (`.github/ISSUE_TEMPLATE/claim.md`); a results README may also read `unadjudicated` until it is adjudicated;
- a rule ratified after its window opened makes the row post-hoc, derived and never written (`scripts/check-results.py`, `post_hoc`);
- a results directory's contract is `results/_template/`, and `results/AGENTS.md` governs what goes in it;
- a rule's bars are checked against what its instrument can attain before a verdict (ruled on #17; operative from 2026-10-13): each clause declares its bar beside the best value a perfect scorer reaches and the values trivial scorers reach on the same pooling, its floor's stance on the predicted outcome, how finely its statistic moves and the chance a trivial scorer reaches the bar anyway, its margin read as an interval, a cell veto's reach, the share of constructed rows, which scorers chose the rows and what the scorer sees, and how well each labelled class's labellers agree; `scripts/check-results.py` holds them to seven checks, and holds a doc line that calls a linked record dead, closed, disproven, ruled out or refuted to that record's word (`results/AGENTS.md`).

`[unsettled]` Planning proposed three more claim rules: parameters outside the recommended range make a row a characterisation; controls seen red before the run; one fire per row, the first. None of them is written in an operative file.

## With a result

| row | lever | word | where |
| --- | --- | --- | --- |
| sense bakeoff (#24) | embedding tier | inconclusive: no cell clears all three bounds over authored sense-descriptions; the best-separated (bge-small) clears the smaller margin over the MiniLM floor. The word was decided by a one-row control veto, and the vetoed gemma raw-cosine cell clears all three bounds on recomputation; the pre-gate's sub-verdict went against the gate and is uninformative, the register having been mined by the gate's own seeds (erratum in the directory) | `results/2026-09-17-sense-bakeoff-adjudication/` |
| entry-to-turn nomination (#17) | fork delivery · supersession | refuted under its rule, twice on one dataset: the deciding clause compares the contenders with MiniLM (margin 0.142, 95% CI −0.02 to 0.32); 102 of 110 positives planted; real-reversal AUC 0.71–0.77 (errata in both directories) | `results/2026-09-19-entry-to-turn-nomination-adjudication/`, `results/2026-09-20-entry-to-turn-nomination-v2-adjudication/` |
| false-nomination framing (b) and (b)-v2 (#89) | fork delivery | refuted, both draws, under a floor clause that fires on the predicted outcome; advisory changed fewer false-nomination turns than imperative on all 12 rungs (errata in both directories) | `results/2026-09-20-false-nomination-framing/`, `results/2026-09-21-false-nomination-framing-v2/` |
| (b′) edit rate (#114) | fork delivery | supported: imperative 53/99, advisory 1/99, sham 0 | `results/2026-09-27-false-nomination-edit-rate/` |
| (b′) on Flash-Next (#142) | fork delivery × substrate | supported; comparison `substrate_independent` on a point estimate: the paired-fork 95% interval of the difference, −0.125 to 0.181, contains the 0.15 dependence margin, so independence is not shown (erratum in the directory) | `results/2026-09-29-false-nomination-edit-rate-second-substrate/` |
| parity fire `extraction-acceptance-inverts` (#115) | admission | supported, inside the band | `results/2026-09-25-extraction-acceptance-parity/` |
| the same parity fire on the candidate rung | admission × substrate | supported; comparison inconclusive, its sign negative | `results/2026-10-01-extraction-acceptance-parity-candidate/` |
| judge seat, zero-shot (#165 → #499) | decision model | refuted for the zero-shot model (26.7% four-way, near chance); the Sonnet judges agree (Fleiss κ 0.886 and 0.969) | `results/2026-10-03-laya-calibration-bound/` |
| judge seat, state lengths (#165) | decision model | refuted: the hypothesis that the archive's 90th-percentile judge state fits laya-en's state budget under both of #165's questions | `results/2026-10-02-judge-state-lengths/` |
| confabulation on nulls (the predecessor's notebook era) | fork warrant | the predecessor's, not a result: it reported that a generative interview fired where nothing capture-worthy happened confabulates an entry in most calls | `results/2026-07-29-confabulation-on-nulls/` |
| extraction acceptance inverts (the predecessor's notebook era) | admission (the archived row the parity fires reproduce) | the predecessor's, not a result | `results/2026-08-10-extraction-acceptance-inverts/` |
| depth probe on the 3.6 floor (#143, an admission cell) | substrate | `pass`: no cliff at 0.5, 0.9 or 0.95 of the 160,000-token pool (deepest 151,622 tokens) | `substrates/admission/accel24-beellama-qwen27b-q4kxl/7c254834cc2c/depth/` |

## Wanted, by the piece each waits on

| row | lever(s) | instrument | waits on |
| --- | --- | --- | --- |
| **reasoning effort** (the maintainer, 2026-10-09: "one of the more common things that I imagine we'll be testing") | reasoning state | the same trajectory at xhigh / medium / low, one effort per session. Forks always inherit the trunk's effort: a different effort changes the system prompt, so a fork would miss the warm prefix it exists to reuse | #531 (landed) |
| **compaction depth 0-1-n** | compaction depth | the same trajectory refilled at none / one / n / total; the second half's success and the receipt | the meditation (#493, landed) and the v0.2.0 session |
| supersession at the meditation (#132) | meditation · supersession | the ratify ask over the register's mined reversals; recall vs the collector's ~0 | the meditation's audit (#504) |
| model-elected purge (#149) | tool-output disposition | a true advisory nudge in the tool-result envelope vs policy-forced vs none vs sham | forks (the tool loop, #298, has landed) |
| "how long" (tool-call duration) | approval · cache lifetime | a typed score from (command, repo, history); labels free from the logged timings | timings in committed results |
| token carriage as a receipt number (#31) | tool-output disposition | sum over tool outputs of size × later requests carrying it | T1's recording |
| the fork economics simulation | cache lifetime | fork reads at measured prefix sizes × window compliance, over the archived message history | no box; a notebook |
| the judge seat, fine-tuned (#499) | decision model | agreement with the Sonnet majority vs the zero-shot baseline; calibration within the bound | real trajectories with the decisions in them |
| the embedding shortlist vs MiniLM | embedding tier | the sense register; pairs/STS on short technical English | after the first drive; no box |
| mimicry and capture under native tool calls (#20, #21) | forks | the first driven sessions' typed outcomes | T1 |
| the tool-shape census (#80) | distribution matching | the same operation under several signatures; well-formed-call rate | nothing: the tool loop (#298) has landed |
| T1 vs T3 across substrates | substrate | the same trajectory on the floor and in the archived Sonnet 5 session | the adapter (#500, #501) |
| one claim re-fired on all three rungs (#394) | substrate | an existing claim under a pre-registered three-rung rule | the bottom rung's admission and #396 |
| placement vs pruning at equal VRAM (#199) | substrate format | heat-mapped placement of Q2_0 against the Coder build; pre-registration follows the routing profile | a routing profile from the archive's drives |
| the fleet runoff (V100 / MI100 / PVC) | substrate | admission within a declared tuning window; loaded cost per successful task | the cards |

A wanted row with an open ticket names it. A row without one gets a ticket when its piece exists and someone means to run it.

## The problems, for anyone who knows a method we do not

Planning's second edition of the problems on #24 ([comment 5972991175](https://github.com/ironblock/discipline/issues/24#issuecomment-5972991175)) states eleven, each as input → output, with its labels, its budget and what was refuted. The constraints that govern all of them:
- nothing runs on the executor's GPU;
- each decision in a session takes at most about 100 ms;
- the text is English, code-mixed;
- anything that reaches the model needs precision over recall;
- mechanical rules come first, and a model handles the residual.

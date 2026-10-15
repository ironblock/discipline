+++
hypothesis = "a fixture claim"
result = "supported"
kind = "reproducible-by-config"
product_sha256 = "5555555555555555555555555555555555555555555555555555555555555555"
controls_run = ["shuffled-label-null"]
known_defects = []
claim_issue = "900"
rule_ratified = { comment = "1", at = "2026-10-15T08:00:00Z", digest = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }
window_start = "2026-10-16T10:00:00Z"
window_start_from = "the fixture"
seen_red = [{ control = "shuffled-label-null", at = "2026-10-15T09:00:00Z", evidence = "controls/shuffled-label-null.red.json", sha256 = "a70e86325d8d6581b66e78b2cee7ae6cb90817577e7b44454e8f5d78620f2ca1" }]

[[ambient]]
instrument = "reader"
harness = "a subagent of the coding agent"
instruction_files = [{ path = "AGENTS.md", sha256 = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff" }]
system_line = "You label each segment with one category."
tools = ["Read"]
memory = "none"
probe = { evidence = "ambient/reader.probe.json", sha256 = "c36123d99fb743cd86eef0a84bbd5d49cb0f8ba5d57dec4ab3de76332847ae06" }

[regime]
arm = "fixture"
substrates = ["fixture-carded"]
dogma_version = 0
+++

# A fixture

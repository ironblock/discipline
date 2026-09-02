---
name: Work
about: Something to build or change that is neither a claim nor a defect. Acceptance is a command and its exit code.
title: "work: "
labels:
---

## Context

<!-- What this is and why it exists, in words a reader who has never seen
     this repository can follow. If the work carries a finding from earlier
     research, state the finding itself -- the number, the failure, the
     lesson -- not a pointer to where it was recorded. Nothing from a private
     environment; the hygiene gate rejects hostnames, addresses, home paths
     and internal ticket identifiers for a reason. -->

## Design

<!-- What gets built, where it lives, and which convention it follows
     (one grammar per format; one authorized implementation; results
     directories carry their own front-matter; and so on). Name the files
     or crates that change. -->

## Acceptance

<!-- REQUIRED. A command and the exit code it must produce once this is
     done. "The feature works" is not an acceptance criterion; a command is.
     Where the work adds a gate or a format, include the seeded fault that
     must turn it red -- a check that has never been seen red is not a check.

     Example:
       cargo test -p discipline-diet -- formats::decline    exit 0
       ./verify.sh --only regimen                            exit 0
       (seeded non-conforming fixture)                       exit 1
-->

| | |
| --- | --- |
| command | `` |
| required exit code | |

## Not in scope

<!-- Optional, but recommended: what this deliberately does not do. An
     unstated boundary is a gap that gets filled by whoever picks the work
     up, and the fill gets recorded as if it were the design. -->

## Provenance

<!-- Optional. The prior evidence this inherits, stated as content. If the
     evidence is data, it belongs in results/ or a fixture directory in this
     repository, not in a link to somewhere a reader cannot open. -->

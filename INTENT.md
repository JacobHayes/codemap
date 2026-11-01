# codemap: Intent

Why codemap exists, what it is for, and what it refuses to become. The other documents say how;
this one says what and why. When a proposal conflicts with this document, this document wins or
is changed on purpose.

## Purpose

codemap is a profiler for code mass and structure. It indexes a codebase and shows where its
size and its accidental complexity lie, so that a person or a coding agent knows where to look
and what to cut.

Codebases now grow faster than people read them. Agents write more code than they need: more
handling, more tests, more layers, more documents, the same idea built twice. Each addition is
reasonable on its own; together they bury the essential parts. codemap makes that mass visible,
the way a disk-usage map makes a full disk legible, and ranks the places most worth a closer
look.

## Who it serves

- **A person exploring.** Open the map, see where the lines and tokens go, drill into an area,
  pivot to another view of the same lines, and read the source behind a finding.
- **A coding agent triaging.** Ask for an overview, take the top areas, investigate them with its
  own tools, and decide what to change.

Typical use is a periodic run (weekly, or before a cleanup) or an interactive session. codemap
is not part of the edit loop.

## What it does

1. **Attributes every line.** Every line of every indexed file is attributed along many
   dimensions: path, language, symbol, role (production, test, docs, config, generated...),
   concern (core logic, error handling, validation, logging, assertion, boilerplate...), and
   whatever analyses add.
2. **Projects.** Every view is a projection of those lines: filter by any dimensions, group by
   one, drill in, pivot to another dimension inside what you drilled into. Ratios such as
   test:code or handling:happy-path are computed, not stored.
3. **Detects.** Detectors flag areas of interest: outliers, concentrations, repetition, stacking
   and disproportion.
4. **Ranks.** Areas are ranked by opportunity: how many independent signals overlap there, times
   how much mass is at stake.

## Principles

- **Point, don't diagnose.** codemap says "look here, and here is why". The person or agent
  investigating decides whether it is a problem and what to do. No verdicts, no fixes.
- **Important over confident.** A missed opportunity costs more than a false lead, which a
  reader dismisses in seconds. Confidence breaks ties; it never hides a finding.
- **Size is a signal.** Mass is not a confounder to normalize away. Too much code, too many
  tests, too much handling and too many words are findings in their own right.
- **Aggregates first.** Overviews summarize the whole; detail is a drill-down away. An overview
  never shows a truncated list as if it were the whole.
- **Global first.** The state of the whole codebase comes first. Changes over time are a later
  addition, never the main view.
- **Generic over specific.** Detectors work on a language-neutral model of lines, symbols and
  references. A language contributes a mapping into that model, not rules of its own.
  Language-specific detectors may come later, which is why extending codemap must be cheap.
- **Deterministic first, inference sprinkled in.** Structure, size and similarity are computed.
  Models are used where meaning matters (semantic duplication, classification, naming), and
  their answers are cached.
- **Essential complexity is acknowledged, not argued with.** In a healthy codebase codemap will
  flag areas that are complex on purpose. Those are a sign it is looking in the right places. A
  project records them as acknowledged, and they stay visible but rank below the rest.
- **The project sets its own bar.** By default codemap compares a codebase with itself
  (outliers, concentrations). A project may add its own thresholds, such as "flag any function
  with more than 100 lines of error handling".
- **codemap must not suffer what it detects.** It stays small: a framework and a few generic
  detectors. Every feature earns its place against the 80:20 cut.

## What it looks for

Accidental complexity in the large shows up as **repetition** (the same intent built more than
once), **stacking** (layers that pass through, wrap, or compensate for other layers) and
**disproportion** (far more handling, tests, configuration or prose than the code they serve).

Detectors are ranked above or below the fold. Above the fold is what codemap ships first and
leads with:

1. **Mass and composition**: where lines and tokens go, by role and concern.
2. **Size and concentration**: outlier files and symbols, and areas holding a disproportionate
   share of the mass.
3. **Duplicated intent**: exact, near and semantic duplicates, across files and languages.
4. **Parallel structures**: naming families (`Foo`, `FooV2`, `LegacyFoo`) and the same set of
   cases matched in many places in ways that look like a table, an accessor or a missed case.
5. **Indirection**: wrappers, pass-through chains and abstractions with a single
   implementation.
6. **Handling volume**: lines and blocks of error handling and defensive code, and where they
   pile up. The concern is too much handling, not missing handling.
7. **Test mass**: test-to-code ratios, duplicated tests (exact and semantic), and tests that
   restate the implementation's literals.
8. **Prose mass**: the volume of documentation and agent instructions. Size alone is often
   reason enough to look.

Below the fold, and possible later: change coupling from history, exclusive footprint
(what would go if a module went), invariant inventory (assertions, contracts, property tests),
configuration and flag surface, dependency surface, concept dispersion, cognitive complexity,
build and test time, imported runtime data (coverage, profiles, test durations), project-defined
judgments by a model (for example "is this a good test?" against the project's criteria), and
comparisons against a baseline.

## Not codemap

- **Not context for agents.** codemap does not feed code to agents during development, retrieve
  snippets, or build a navigation index. Agents use it to find work, not to do it.
- **Not a navigator or code search.**
- **Not a linter.** No per-language style rules, no nitpicks, no autofix.
- **Not a verdict.** No grades, no pass/fail gates, no health score to optimize.
- **Not provenance.** Who or what wrote a line does not matter; what is there does.
- **Not a runtime.** codemap never runs the code it indexes. Runtime data may be imported from
  files other tools produce.
- **Not a history miner.** Version control history is not needed. It may become one optional
  input later, never a requirement.
- **Not a service.** It runs locally, on demand. No hosting, no schedules, no multi-repo
  dashboards.
- **Not a pull-request reviewer.** Diffs against a baseline may come later; the whole codebase
  is the subject.

## Success

On codebases its users know well, the top areas codemap ranks are ones they would want
investigated, including some they had not spotted, and the acknowledged ones are areas they
knew to be complex. An agent given only codemap's output finds real simplifications sooner than
one exploring alone.

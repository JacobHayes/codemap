---
name: codemap
description: Find where a codebase carries accidental complexity (outsized files, piled-up handling, test or prose mass) with the codemap CLI, then investigate and decide what to cut. Use for periodic cleanups or when asked where a codebase has too much code.
---

# codemap triage

codemap indexes a codebase and ranks areas worth a closer look. It points; it does not
diagnose. You investigate each area with your own tools and decide whether it is a problem.
It never runs the code and is not part of the edit loop. The query commands used here
(`overview`, `tree`, `xtab`, `areas`, `area`, `source`) run from the project root, or take `-C <root>`, and
take `--json` for the structured answer; use it when you will process the output. If your
client speaks MCP, `codemap mcp` serves the same queries as tools (overview, tree, xtab, areas,
area, source, catalog) with the same arguments and answers.

## 1. Setup (first run in a project)

Roles (prod, test, docs, config, script, bench, example, generated, vendored) come from path
conventions. Generated and vendored lines are never ranked; every detector compares a measure
against the lines of the role it belongs to. A wrong role therefore skews every ranking, so
fix roles once before trusting the areas.

1. `codemap overview`: read the role breakdown. Large generated, config or test shares
   deserve a look.
2. Find the top-mass files: `codemap tree --depth 2` for the whole index, then
   `codemap tree --role <role> --depth 2` for each role, narrowing with `--path <dir>`. Read
   the biggest files of each, because the largest masses in a codebase are often not its code.
   Mark what is not the project's own code: vendored or bundled third-party source, generated
   code with no marker, snapshots and data (test vectors, fixtures, fonts, tables) that are
   not tests, copied-in code. Test doubles and fakes that live outside test directories (a
   `FakeHost` in `src/`) belong in `[roles]` as `test`, or they count as production
   implementations. Size findings count only files in a recognised programming language, so
   data files do not rank, but they still skew the role breakdown.
3. Record fixes in `.codemap/config.toml`. Keys are gitignore-style globs, values are role
   names; the first match wins:

   ```toml
   [roles]
   "api/openapi.json" = "generated"
   "third_party_src/" = "vendored"
   "testdata/" = "test"
   ```

4. Re-run `codemap overview` and confirm the breakdown now matches the project.
5. When the user says an area is complex on purpose, acknowledge it. It stays visible but
   ranks after the rest. Always record their reason; `detector` is optional.

   ```toml
   [[acknowledge]]
   path = "src/parser"
   detector = "size"
   reason = "hand-written parser tables"
   ```

   A group (copies, a naming family, a repeated switch) is acknowledged by its id, which
   `codemap areas` prints under it, with `group = "group:duplication:..."` in place of `path`
   and `members = [...]` as `codemap area <id>` lists them. The acknowledgement follows the
   group while its copies are edited or its sites come and go. A `path` covers a group only when
   every member lies under it.

To find duplicates that share intent in different words, add an `[inference]` table and put
the key in the environment (OpenRouter's by default). The empty table is enough: it uses Jev as
the decision model, which separates copies from look-alikes well and costs a few cents per
thousand candidates, and Qwen3 Embedding 8B for the vectors.

```toml
[inference]
```

`embedding_model`, `decision_model`, `base_url` and `api_key_env` override the defaults.

Commit `.codemap/config.toml` with the project; it holds facts about the project, not about
your session.

## 2. Triage

1. `codemap areas --top 15 --json`: ranked areas, each with the detectors that flagged it and
   why. Narrow with `--path <dir>`, `--detector <name>` or the lines the findings touch:
   `--role test`, `--concern handler`, `--language rust`; `codemap catalog` lists
   detectors, measures and dimensions.
2. `codemap area <subject> --json`: the findings, evidence (line ranges) and related areas
   for one file or directory, or one group id, exactly as `areas` printed it. A group (copies,
   a naming family, a repeated switch) is one area however many files it spans; asking for a
   file or directory also lists the groups it belongs to and its share of each, and every open
   area with evidence inside it (`touching`, with the lines there). `codemap tree --color
   finding_share` colours files and folders by the share of their lines inside open findings
   (`finding_lines` is the count), so a pattern spread over many
   files shows on every one.
   `codemap source <path> --lines a-b` shows those lines with role, concern and the findings
   touching them (long files come in pages; it says how to ask for the next).
   `codemap xtab --rows role --cols concern` crosses two dimensions (role, concern, language,
   kind, path) with a measure per cell, to see composition at a glance. `tree` and `xtab`
   leave generated and vendored lines out and say how many; `--include-generated` counts them.
3. Read the evidence with your own tools; a duplication finding lists every copy, so open each
   and compare. A finding that says the group was found by meaning comes from a model: the
   copies share intent in different words, so read them for that, and expect some to be a
   function beside its test or a wrapper over its callee. That analysis is off unless
   `.codemap/config.toml` has an `[inference]` table and the key is in the environment;
   `codemap index` says in one line whether it ran. Its first run asks about every candidate
   and costs money (the index line prints the spend); later runs reuse the cached answers. Ask: is this mass needed? Is the same idea built
   twice? Are there wrappers or chains that only pass through? Is the handling or testing out of
   proportion to what it protects?
4. Decide. Either propose a concrete simplification, or, if the complexity is essential, tell
   the user and (with their agreement) add an `[[acknowledge]]` entry.

## 3. Rules of thumb

- A finding is a lead, not a verdict. Dismiss false leads quickly and say why.
- Size is a signal in itself: a very large file, test suite or document is worth a look even
  when it is tidy.
- If a top area is something nobody can cut by editing (a generated or vendored file), fix
  its role in `[roles]`; do not argue with the ranking.
- Report the few areas that matter with the evidence you read, not the whole list.

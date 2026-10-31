# codemap: for agents

You are changing codemap. This file is the entry point; the documents it names are
authoritative.

## Read before writing anything

1. `INTENT.md`: what codemap is for and what it is not. Every change serves it.
2. `ARCHITECTURE.md`: how it is built. Do not reopen its decisions in passing; changing one is a
   deliberate call that rewrites the document.
3. `PRACTICES.md`: how code is written and checked.
4. `DESIGN.md`, if the change touches the UI or the terminal output.

## Rules

- **The documents win.** If code contradicts a document, follow the document or change it on
  purpose in the same commit. Each rule is said once, in the document that owns it.
- **Write current state, never history.** Documents and comments say how things are; how they
  came to be belongs to version control.
- **Stay above the fold.** If a change grows codemap beyond `INTENT.md`'s above-the-fold list,
  stop and raise it instead.
- **Done means checked and used.** `mise run check` passes, and you have run the change the way a
  person or agent would.
- **Version control is jj**, colocated with git. Commit only your own files.
- **Commit messages:** a short imperative title, and a body only when the why is not obvious.

## Reviewing a change

The author has already run `mise run check` on it, so don't rerun the check, full builds or
code generation. Run any narrower command that helps confirm a suspicion: one test, a query, the
binary on a fixture. Judge the change against the documents above; mechanical rules are the
linters' job.

## Talking with people

Plainly and on the merits: give the current state, recommend one option, and push back with the
concrete tradeoff when you disagree.

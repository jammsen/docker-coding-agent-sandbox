# OpenCode Sandbox Rules

All projects are located under `/home/opencode/workspace`. Work only inside this directory, with no exceptions, unless the user explicitly asks for access outside the sandbox.

When starting work in an existing project directory, check whether `WORKLOG.md` exists and read it for prior context. Commands that change files define their own required worklog steps; follow the command workflow exactly.

Create a new subdirectory under `/home/opencode/workspace` only when the user asks for a new standalone task or project. If an existing project directory might match the request, ask the user to confirm whether to use it, and recommend a clear directory name before creating anything new. Do not create nested duplicate project directories.

Prefer focused changes over broad rewrites. Extend existing behavior through the smallest fitting change before replacing working code. Before large refactors, first produce a short audit and wait for the user to choose what should be changed.

## Sandbox Commands

Use the mounted sandbox commands for repeatable workflows when they fit the task:

- `/refactor-audit <target>`: inspect the named file, directory, symbol, or current git diff for refactor opportunities without editing files.
- `/refactor-apply <approved scope>`: apply one focused refactor after the user has approved the scope, then verify and update `WORKLOG.md`.
- `/git-commit`: review, document, and commit approved changes using Conventional Commits.

Do not treat commands as mandatory for every task. They are shortcuts for user-invoked workflows and should not replace direct, focused work when the user has already given a clear instruction.

## Skills

Use the `write-worklog` skill only when a detailed `WORKLOG.md` entry is needed or the user asks for worklog formatting. For command-driven workflows, prefer the worklog format embedded in the command itself.

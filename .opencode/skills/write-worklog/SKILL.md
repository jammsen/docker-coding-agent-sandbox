---
name: write-worklog
description: Maintain a WORKLOG.md file in the project directory to track task progress, decisions, and outcomes. Use when completing tasks, making file changes, or working on multi-step project work.
---

# Write Worklog

Maintain a file called `WORKLOG.md` in the current project working directory at all times. This logs every task you complete, keeping context available for later sessions.

## Core rules

### Before starting a task

1. Read `WORKLOG.md` to understand prior context and what is still pending.
2. If the file does not exist, create it before doing anything else.

### After completing a task

1. Append a new entry to `WORKLOG.md` immediately.
2. Do NOT proceed to the next task until the entry is written.

### Workflow checklist

Copy this checklist and track progress before moving on:

```
Worklog:
- [ ] Read existing WORKLOG.md
- [ ] Append task entry (see format below)
- [ ] Include files changed and exact lines modified
- [ ] Add concrete findings, not vague summaries
- [ ] List pending items
- [ ] Verify entry is written before continuing
```

## Entry format

Each entry MUST include all of the following:

- Task heading (descriptive summary)
- **Status** and **Date** (DD.MM.YYYY, HH:MM - CET/CEZ, Europe/Berlin)
- **What was done** (files changed, exact lines modified)
- **What was found** (issues, bugs, observations with concrete specifics)
- **Pending** (outstanding items or follow-ups)

Use this structure:

```markdown
---

## <task-description>

**Status:** Done | In Progress | Pending
**Date:** DD.MM.YYYY, HH:MM (CET/CEZ)

### What was done

- File: `path/to/file` — changed / created / removed
- Specific detail of what was modified
- Exact lines or sections if applicable

### What was found

- Concrete issues, bugs, or observations
- Specific file paths and line numbers

### Pending

- Remaining items that still need attention
- Any blockers or follow-ups
```

## Do not

- Do NOT proceed to the next task without updating the worklog.
- Do NOT write vague summaries — include exact details.
- Do NOT skip entries, even for small tasks.
- Do NOT use relative time references without timestamps.

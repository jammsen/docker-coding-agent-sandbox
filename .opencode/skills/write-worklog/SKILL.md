---
name: write-worklog
description: Append structured WORKLOG.md entries for task progress, decisions, and outcomes. Use when a project requires a detailed worklog entry or the user asks for one.
---

# Write Worklog

Append structured entries to `WORKLOG.md` in the current project working directory. This keeps concrete task context available for later sessions.

## Core rules

### Before starting a task

1. Read `WORKLOG.md` if it exists to understand prior context and pending work.
2. If the file does not exist, create it when a worklog entry is required.

### After completing a task

1. Append a new entry to `WORKLOG.md`.
2. Keep the entry concise and specific.

### Workflow checklist

Use this checklist:

```
Worklog:
- [ ] Read existing WORKLOG.md
- [ ] Append task entry (see format below)
- [ ] Include files changed and exact lines modified
- [ ] Add concrete findings, not vague summaries
- [ ] List pending items
- [ ] Verify entry is written
```

## Entry format

Each entry MUST include all of the following:

- Task heading (descriptive summary)
- **Status** and **Date** (DD.MM.YYYY, HH:MM - CET/CEST, Europe/Berlin)
- **What was done** (files changed, exact lines modified)
- **What was found** (issues, bugs, observations with concrete specifics)
- **Pending** (outstanding items or follow-ups)

For the timestamp, use:

```bash
TZ=Europe/Berlin date "+%d.%m.%Y, %H:%M (%Z)"
```

Use this structure:

```markdown
---

## <task-description>

**Status:** Done | In Progress | Pending
**Date:** DD.MM.YYYY, HH:MM (CET/CEST)

### What Changed

- File: `path/to/file` — changed / created / removed
- Specific detail of what was modified
- Exact lines or sections if applicable

### Findings

- Concrete issues, bugs, or observations
- Specific file paths and line numbers

### Pending

- Remaining items that still need attention
- Any blockers or follow-ups
```

## Avoid

- Do NOT write vague summaries — include exact details.
- Do NOT use relative time references without timestamps.

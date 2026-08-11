---
name: brain-skills
description: Create and maintain project-scoped skills for the selected brain root.
---

# Brain skills

Brain skills are project-scoped. They belong to the selected brain root, not to
your machine-wide agent configuration.

## Create a skill

When the user asks you to create a skill for this brain, create a directory at:

```text
<brain-root>/.agents/skills/<skill-name>/
```

Put the required `SKILL.md` there, alongside any scripts or reference files the
skill needs. Keep the skill generic unless the user explicitly asks for
workspace-specific behavior. Do not write it to `~/.agents/skills`,
`~/.claude/skills`, `~/.codex/skills`, or a global OpenCode skills directory.

After creating or editing the skill, run:

```sh
brain skills sync
```

That command preserves user-authored skills under `<brain-root>/.agents/skills`
and creates the project-local links used by Claude, Codex, and OpenCode:

```text
<brain-root>/.claude/skills/<skill-name>
<brain-root>/.codex/skills/<skill-name>
<brain-root>/.opencode/skills/<skill-name>
```

The links are recreated safely and can be refreshed at any time by running the
same sync command again.

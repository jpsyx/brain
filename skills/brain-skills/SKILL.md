---
name: brain-skills
description: Author and maintain plugin skills or extensions in the selected Brain workspace, then sync their generated frontend copies.
---

# Brain skills

Brain customizations belong to the selected workspace. Prefer durable plugin
sources under `.config/plugins`; generated copies are not the editing target.
Do not install a workspace skill into a global agent configuration.

<a id="create-a-skill"></a>

## Create or update a plugin

Create a new workspace-owned skill at:

```text
<brain-root>/.config/plugins/<skill-name>/SKILL.md
```

Use matching lowercase-hyphen names and concise `name`/`description` frontmatter.
Describe when it applies, preserve the user's intended scope, and keep only
workflow-specific guidance in the entrypoint. Put substantial conditional
procedures in linked references; include scripts or assets only when needed.
Do not collide with an existing bundled or user-owned skill name.

Edit the durable source, then run `brain skills sync`. It renders the plugin
into `<brain-root>/.agents/skills/<skill-name>/` and refreshes project-local
Claude, Codex, and OpenCode links. Brain's frontend integrations expose the
selected workspace's skills; do not create a second implementation per frontend.
Inspect the rendered result and verify referenced files and any scripts.

Existing **unmarked user-authored directories** under `.agents/skills` remain
supported and are preserved by sync. Do not overwrite, rename, or relocate
them as an incidental part of creating a plugin. If the user explicitly asks
to adopt one into `.config/plugins`, preserve its complete contents and resolve
any destination collision first. Never edit Brain-managed rendered copies as
persistent source; the next refresh regenerates them.

## Extend a bundled workflow

Put workspace customization in `.config/extensions/<skill-name>.md` using the
hooks declared by that workflow. Use `.config/extensions/workspace.md` for
shared context under `[workspace:context]`, and the focused owner for
domain-specific additions. The workspace `AGENTS.md` is fully generated: do not
customize it directly or append rules outside its managed markers. Reconciliation
replaces the entire document from core and extension sources. New
resource-manager additions target `organize:reference-manager`; legacy
`second-brain:reference-manager` remains a supported alias. Existing source
bytes remain user-owned; sync projects aliases without rewriting their files.
Unknown or catch-all content is retained by the compatibility compiler.

Read the owning skill before adding a hook-specific instruction. For example,
[organize](../organize/SKILL.md) owns resource workflows and
[projects](../projects/SKILL.md) owns project lifecycle. Sync after editing,
check for reported conflicts, and leave original sources intact when a
publication is refused. No customization is required for bundled core to work.

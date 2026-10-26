---
name: second-brain
description: Compatibility routes for existing second-brain invocations and links. Use projects, process-capture, organize, or brain-sync for new requests.
---

# second-brain

This compatibility router forwards the requested operation to its focused
owner. Load only the owner needed for the request, respecting trusted capability
availability. If the required capability is unavailable, explain the limitation;
do not bypass access restrictions. Shared workspace and response policy lives
in the selected workspace's `AGENTS.md`, loaded by the installed skill prelude.

## Commands

| Existing request or invocation | Canonical owner |
| --- | --- |
| `/second-brain add-project`, "add/start a project" | [projects create](../projects/SKILL.md#create) |
| `/second-brain complete-project`, "mark project as done" | [projects complete](../projects/SKILL.md#complete) |
| `/second-brain archive-project`, "archive this project" | [projects archive](../projects/SKILL.md#archive) |
| `/second-brain extract-ip`, "extract intermediate packets" | [extract IPs](../projects/references/extract-ips.md) |
| `/second-brain process-capture`, "process capture", "move this into my brain" | [process-capture](../process-capture/SKILL.md) |
| `/second-brain add-resource`, "save this", "where does this note belong?" | [organize](../organize/SKILL.md#add-a-resource) |
| `/second-brain find`, "get/fetch/find items" | [organize retrieval](../organize/SKILL.md#retrieval) |
| `/second-brain reindex`, "rebuild the lookups" | [organize reindex](../organize/SKILL.md#reindex) |
| `/second-brain cloud-sync`, "sync", "do a sync", "sync my brain" | [brain-sync cloud sync](../brain-sync/SKILL.md#cloud-sync) |
| `/second-brain resolve-conflicts`, "resolve sync conflicts" | [brain-sync conflict resolution](../brain-sync/SKILL.md#resolve-conflicts) |
| Contact lookup or maintenance | [contacts](../contacts/SKILL.md) |

Bare sync means cloud synchronization. Local reindex is a separate explicit
request. Conversation knowledge goes to
[brain-knowledge-capture](../brain-knowledge-capture/SKILL.md); source summaries
use [article-summarizer](../article-summarizer/SKILL.md).

## Retained link destinations

Historical anchors below forward to their current owner. They do not add a
second copy of shared policy or of a workflow.

<a id="personal-assistant-mode"></a>
<a id="inbound-brain-messages-sms-and-email"></a>
<a id="sending-a-longer-response-by-email"></a>
<a id="naming-conventions"></a>
<a id="referencing-markdown-files"></a>
<a id="always-end-with-an-additions-table"></a>
<a id="end-of-session-clean-up-tool-byproducts"></a>

### Workspace conventions

Follow the selected workspace's `AGENTS.md` for actor identity, common naming,
response presentation, delivery authorization, and cleanup.

<a id="core-principle"></a>
<a id="the-four-buckets"></a>
<a id="areas--ongoing-responsibilities"></a>
<a id="resources--reference-and-curiosity"></a>
<a id="archive--inactive-but-preserved"></a>
<a id="decision-flow"></a>
<a id="when-something-doesnt-fit"></a>
<a id="common-mistakes"></a>

### Placement

Continue with [organize placement](../organize/SKILL.md#placement).

<a id="projects--outcome-driven-finite"></a>
<a id="namespaces"></a>
<a id="every-project-has-a-metadatajson--readmemd"></a>
<a id="add-a-new-project--start-a-project-for-x"></a>

### Project creation and records

Continue with [projects](../projects/SKILL.md#create) and its
[metadata reference](../projects/references/metadata.md).

<a id="intermediate-packets-ips"></a>
<a id="extract-ip--extract-intermediate-packets-from-x"></a>

### Reusable project work

Continue with [IP extraction](../projects/references/extract-ips.md).

<a id="working-with-tasks-linked-to-projects"></a>

### Project task links

Continue with [task-project linkage](../projects/references/task-project-link.md).

<a id="mark-this-project-as-complete--mark-project-as-done--this-project-is-done"></a>

### Project completion

Continue with [projects complete](../projects/SKILL.md#complete).

<a id="archive-this-project--im-done-with-x"></a>

### Project retirement

Continue with [projects archive](../projects/SKILL.md#archive), including its
[task removal contract](../todo/SKILL.md#remove-tasks).

<a id="capture--the-users-in-basket-not-a-para-bucket"></a>
<a id="rules-for-capture"></a>
<a id="a-file-is-not-a-note"></a>
<a id="not-everything-in-capture-is-a-note"></a>
<a id="process-capture--move-this-into-my-brain"></a>

### Capture processing

Continue with [process-capture](../process-capture/SKILL.md).

<a id="add-a-resource--save-this-pdf-image-plaintext-notes"></a>
<a id="paired-subdirectories-for-pdfs--media-with-notes"></a>
<a id="cross-link-new-notes-see-also"></a>

### Resources and links

Continue with [organize resources](../organize/SKILL.md#add-a-resource) and
[paired media/notes](../organize/references/resources.md#paired-media-and-notes).

<a id="get--fetch--find-items--search-and-retrieval"></a>

### Find material

Continue with [organize retrieval](../organize/SKILL.md#retrieval).

<a id="reindex-the-second-brain--second-brain-reindex"></a>
<a id="csv-tooling--keep-it-simple"></a>

### Local indexes

Continue with [organize reindex](../organize/SKILL.md#reindex).

<a id="cloud-sync-the-brain--second-brain-cloud-sync"></a>

### Cloud synchronization

Continue with [brain-sync](../brain-sync/SKILL.md#cloud-sync).

<a id="resolve-sync-conflicts--second-brain-resolve-conflicts"></a>

### Conflict resolution

Continue with [brain-sync conflicts](../brain-sync/SKILL.md#resolve-conflicts).

<a id="contacts"></a>

### People and service providers

Continue with [contacts](../contacts/SKILL.md).

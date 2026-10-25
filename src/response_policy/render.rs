use crate::actor::Channel;

use super::policy;

/// Render the presentation rules for the trusted initiating channel.
#[must_use]
pub fn render_instructions(channel: Channel) -> String {
    let selected = policy(channel);
    let template = match selected.channel {
        Channel::Interactive => include_str!("../../templates/workspace/response/interactive.md"),
        Channel::Sms => include_str!("../../templates/workspace/response/sms.md"),
        Channel::Email => include_str!("../../templates/workspace/response/email.md"),
    };
    selected.max_characters.map_or_else(
        || template.to_owned(),
        |limit| template.replace("{{max_characters}}", &limit.to_string()),
    )
}

/// Render all channels from the same inputs used for selected launch context.
#[must_use]
pub fn render_workspace_reference() -> String {
    [Channel::Interactive, Channel::Sms, Channel::Email]
        .map(render_instructions)
        .join("\n")
}

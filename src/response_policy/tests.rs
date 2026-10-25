use super::*;

#[test]
fn sms_policy_is_selected_from_channel_not_request_text() {
    let sms = policy(crate::actor::Channel::Sms);
    assert_eq!(sms.max_characters, Some(480));
    assert!(!sms.markdown);
    assert!(!sms.local_paths);
    assert!(!sms.changes_table);
    assert!(policy(crate::actor::Channel::Interactive).local_paths);
    assert!(policy(crate::actor::Channel::Email).markdown);
}

#[test]
fn workspace_reference_and_launch_policy_share_the_channel_rules() {
    use crate::actor::Channel;

    let reference = render_workspace_reference();
    for channel in [Channel::Interactive, Channel::Sms, Channel::Email] {
        let selected = render_instructions(channel);
        assert!(reference.contains(&selected));
        assert!(!selected.contains("{{"), "unrendered policy placeholder");
        assert!(selected.contains(channel.as_str()));
    }
    let sms = render_instructions(Channel::Sms);
    assert!(sms.contains("480 Unicode characters"));
    assert!(sms.contains("no Markdown"));
    assert!(sms.contains("no filesystem paths"));
    let email = render_instructions(Channel::Email);
    assert!(email.contains("self-contained"));
    assert!(email.contains("no filesystem paths"));
    assert!(email.contains("changes/additions table is not required"));
    let interactive = render_instructions(Channel::Interactive);
    assert!(interactive.contains("final changes/additions table"));
    assert!(interactive.contains("usable file references"));
}

/// Render shared workspace guidance from one authored contract and channel policy.
pub(crate) fn render_contract() -> String {
    include_str!("../../../templates/workspace/AGENTS.md").replace(
        "{{response_policy}}",
        &crate::response_policy::render_workspace_reference(),
    )
}

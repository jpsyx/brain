use super::BrainPanelState;

pub(crate) const fn exhaust_skill_session_tab_ids(state: &mut BrainPanelState) {
    state.next_session_tab_id = u32::MAX;
}

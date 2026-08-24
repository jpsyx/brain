use super::BrainPanelState;

pub(crate) const fn exhaust_session_tab_ids(state: &mut BrainPanelState) {
    state.set_next_session_tab_id(u32::MAX);
}

pub(crate) const fn exhaust_skill_session_tab_ids(state: &mut BrainPanelState) {
    exhaust_session_tab_ids(state);
}

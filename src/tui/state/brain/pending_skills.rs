use super::BrainPanelState;
use crate::skill_session::SkillSessionSpec;

impl BrainPanelState {
    pub(crate) fn has_pending_skill_starts(&self) -> bool {
        !self.pending_skills.is_empty()
    }

    pub(crate) fn defer_skill_start(&mut self, spec: &SkillSessionSpec) {
        if !self
            .pending_skills
            .iter()
            .any(|pending| pending.key == spec.key)
        {
            self.pending_skills.push(spec.clone());
        }
    }

    pub(crate) fn take_pending_skill_starts(&mut self) -> Vec<SkillSessionSpec> {
        std::mem::take(&mut self.pending_skills)
    }
}

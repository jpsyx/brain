//! Skill (re)rendering seam.
//!
//! Any config or personalization mutation must keep the installed brain skills
//! consistent with the user's current values. The real render/install pipeline
//! lands in **sub-project B**; in sub-project A this is a deliberate stub with a
//! single call site (`resync_skills`) so B can fill in the body without
//! touching every mutation path.
//!
//! It must never fail a mutation: a `config set` or `personalize set` succeeds
//! even if a future render step errors, so this returns nothing and swallows
//! (future) failures internally.

/// Re-render and install the brain skills against the current config and
/// personalization. **Stub (sub-project A):** a no-op. Wired into every
/// mutation path now so B only has to implement the body.
pub fn resync_skills() {
    // Intentionally empty until sub-project B. See docs/decisions.md.
}

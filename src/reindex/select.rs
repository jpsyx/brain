//! Pure selection: which parts of a reindex run.
//!
//! Mirrors the documented CLI contract — a bare `brain reindex` rebuilds all
//! three families; any explicit `--projects` / `--resources` / `--tasks` flag
//! narrows the run to just the named families.

/// Which lookup families to rebuild this run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub projects: bool,
    pub resources: bool,
    pub tasks: bool,
}

/// Resolve the flag triple into a selection: none set ⇒ all three.
#[must_use]
pub const fn selection(projects: bool, resources: bool, tasks: bool) -> Selection {
    if !projects && !resources && !tasks {
        Selection {
            projects: true,
            resources: true,
            tasks: true,
        }
    } else {
        Selection {
            projects,
            resources,
            tasks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_flags_selects_everything() {
        assert_eq!(
            selection(false, false, false),
            Selection {
                projects: true,
                resources: true,
                tasks: true
            }
        );
    }

    #[test]
    fn a_single_flag_narrows_to_just_that_family() {
        assert_eq!(
            selection(false, true, false),
            Selection {
                projects: false,
                resources: true,
                tasks: false
            }
        );
    }
}

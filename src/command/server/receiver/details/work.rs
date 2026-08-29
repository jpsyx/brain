use crate::theme::Theme;

use super::{ReceiverStatus, WORK_LABEL_WIDTH, row};

/// Availability of one workspace's read-only durable work snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverWorkState {
    Available(crate::state::ReceiverWorkSummary),
    Unavailable,
}

/// Redacted durable delivery phases for `brain receiver status`. Pure.
#[must_use]
pub(crate) fn delivery_rows(counts: crate::state::ReceiverDeliveryCounts, theme: Theme) -> String {
    let fields = delivery_fields(counts);
    let phases = fields[..6]
        .iter()
        .copied()
        .map(|(phase, count)| format!("{} {}", theme.muted(phase), theme.value(&count.to_string())))
        .collect::<Vec<_>>()
        .join("  ");
    let reasons = fields[6..]
        .iter()
        .copied()
        .map(|(reason, count)| {
            format!(
                "{} {}",
                theme.muted(reason),
                theme.value(&count.to_string())
            )
        })
        .collect::<Vec<_>>()
        .join("  ");
    format!("{phases}\n{reasons}")
}

fn delivery_fields(counts: crate::state::ReceiverDeliveryCounts) -> [(&'static str, usize); 11] {
    [
        ("answer-ready", counts.answer_ready()),
        ("delivering", counts.delivering()),
        ("retrying", counts.retrying()),
        ("ambiguous", counts.ambiguous()),
        ("failed", counts.failed()),
        ("done", counts.done()),
        ("retry-exhausted", counts.retry_exhausted()),
        ("permanent-rejection", counts.permanent_rejection()),
        (
            "ambiguous-acknowledgement",
            counts.ambiguous_acknowledgement(),
        ),
        (
            "idempotency-window-expired",
            counts.idempotency_window_expired(),
        ),
        ("no-safe-fallback", counts.no_safe_fallback()),
    ]
}

/// Content-free durable agent, recovery, cleanup, and delivery work. Pure.
#[must_use]
pub(crate) fn work_rows(work: &ReceiverWorkState, theme: Theme) -> String {
    let ReceiverWorkState::Available(summary) = work else {
        return row(
            "Durable work",
            &theme.warning("unavailable"),
            WORK_LABEL_WIDTH,
            theme,
        );
    };
    let oldest = summary
        .oldest_active_phase()
        .map_or("none", crate::state::ReceiverWorkPhase::as_str);
    let recovery = summary.recovery_attempt().map_or_else(
        || "not active".to_owned(),
        |attempt| format!("{attempt}/{}", summary.recovery_limit()),
    );
    [
        row(
            "Agent queue",
            &theme.value(&summary.agent_queue_depth().to_string()),
            WORK_LABEL_WIDTH,
            theme,
        ),
        row(
            "Oldest phase",
            &theme.value(oldest),
            WORK_LABEL_WIDTH,
            theme,
        ),
        row("Recovery", &theme.value(&recovery), WORK_LABEL_WIDTH, theme),
        row(
            "Cleanup gated",
            &theme.value(&summary.cleanup_gated_responses().to_string()),
            WORK_LABEL_WIDTH,
            theme,
        ),
        delivery_rows(summary.delivery_counts(), theme),
    ]
    .join("\n")
}

/// Compact status shared by the TUI receiver-status action. Pure.
#[must_use]
pub(crate) fn receiver_status_flash(
    status: ReceiverStatus,
    work: &ReceiverWorkState,
    theme: Theme,
) -> String {
    let mut text = format!(
        "receiver {}; TUI {}; server {}; accepting {}",
        if status.enabled {
            "enabled"
        } else {
            "disabled"
        },
        if status.tui_live { "live" } else { "not live" },
        if status.server_running {
            "running"
        } else {
            "not running"
        },
        if status.accepting { "yes" } else { "no" },
    );
    match work {
        ReceiverWorkState::Available(summary) => {
            use std::fmt::Write as _;
            let oldest = summary
                .oldest_active_phase()
                .map_or("none", crate::state::ReceiverWorkPhase::as_str);
            let recovery = summary.recovery_attempt().map_or_else(
                || "not active".to_owned(),
                |attempt| format!("{attempt}/{}", summary.recovery_limit()),
            );
            let _ = write!(
                text,
                "; agent queue {}; oldest {}; recovery {}; cleanup gated {}",
                summary.agent_queue_depth(),
                oldest,
                recovery,
                summary.cleanup_gated_responses(),
            );
            for (label, count) in delivery_fields(summary.delivery_counts()) {
                let _ = write!(text, "; {label} {count}");
            }
        }
        ReceiverWorkState::Unavailable => text.push_str("; durable work unavailable"),
    }
    theme.info(&text)
}

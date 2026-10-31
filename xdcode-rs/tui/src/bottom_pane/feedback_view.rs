use ratatui::style::Stylize;
use ratatui::text::Line;
use xdcode_feedback::DOCTOR_REPORT_ATTACHMENT_FILENAME;
use xdcode_feedback::FEEDBACK_DIAGNOSTICS_ATTACHMENT_FILENAME;
use xdcode_feedback::FeedbackDiagnostics;
use xdcode_feedback::WINDOWS_SANDBOX_LOG_ATTACHMENT_FILENAME;
use xdcode_feedback::XDCODE_APP_DIRECTORY_CACHE_ATTACHMENT_FILENAME;
use xdcode_feedback::XDCODE_APPS_TOOLS_CACHE_ATTACHMENT_FILENAME;

use crate::app_event::AppEvent;
use crate::app_event::FeedbackCategory;
use crate::app_event_sender::AppEventSender;
use crate::history_cell;

use super::popup_consts::standard_popup_hint_line;

/// The target audience for feedback disclosure and follow-up instructions.
///
/// This controls displayed copy and links, not feedback upload behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FeedbackAudience {
    ProviderEmployee,
    External,
}

pub(crate) fn should_show_feedback_connectivity_details(
    category: FeedbackCategory,
    diagnostics: &FeedbackDiagnostics,
) -> bool {
    category != FeedbackCategory::GoodResult && !diagnostics.is_empty()
}

pub(crate) fn feedback_classification(category: FeedbackCategory) -> &'static str {
    match category {
        FeedbackCategory::BadResult => "bad_result",
        FeedbackCategory::GoodResult => "good_result",
        FeedbackCategory::Bug => "bug",
        FeedbackCategory::SafetyCheck => "safety_check",
        FeedbackCategory::Other => "other",
    }
}

pub(crate) fn feedback_success_cell(
    category: FeedbackCategory,
    include_logs: bool,
    thread_id: &str,
    feedback_audience: FeedbackAudience,
) -> history_cell::WebHyperlinkHistoryCell {
    let _ = (category, include_logs, thread_id, feedback_audience);
    history_cell::WebHyperlinkHistoryCell::new(vec![Line::from(
        "Feedback uploads are unavailable in xdcode. Use xdcode doctor for local diagnostics.",
    )])
}

// Build the selection popup params for feedback categories.
pub(crate) fn feedback_selection_params(
    app_event_tx: AppEventSender,
) -> super::SelectionViewParams {
    super::SelectionViewParams {
        title: Some("How was this?".to_string()),
        items: vec![
            make_feedback_item(
                app_event_tx.clone(),
                "bug",
                "Crash, error message, hang, or broken UI/behavior",
                FeedbackCategory::Bug,
            ),
            make_feedback_item(
                app_event_tx.clone(),
                "bad result",
                "Output was off-target, incorrect, incomplete, or unhelpful",
                FeedbackCategory::BadResult,
            ),
            make_feedback_item(
                app_event_tx.clone(),
                "good result",
                "Helpful, correct, high‑quality, or delightful result worth celebrating",
                FeedbackCategory::GoodResult,
            ),
            make_feedback_item(
                app_event_tx.clone(),
                "safety check",
                "Benign usage blocked due to safety checks or refusals",
                FeedbackCategory::SafetyCheck,
            ),
            make_feedback_item(
                app_event_tx,
                "other",
                "Slowness, feature suggestion, UX feedback, or anything else",
                FeedbackCategory::Other,
            ),
        ],
        ..super::SelectionViewParams::picker()
    }
}

/// Build the selection popup params shown when feedback is disabled.
pub(crate) fn feedback_disabled_params() -> super::SelectionViewParams {
    super::SelectionViewParams {
        title: Some("Sending feedback is disabled".to_string()),
        subtitle: Some("This action is disabled by configuration.".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items: vec![super::SelectionItem {
            name: "Close".to_string(),
            dismiss_on_select: true,
            ..Default::default()
        }],
        ..super::SelectionViewParams::picker()
    }
}

fn make_feedback_item(
    app_event_tx: AppEventSender,
    name: &str,
    description: &str,
    category: FeedbackCategory,
) -> super::SelectionItem {
    let action: super::SelectionAction = Box::new(move |_sender: &AppEventSender| {
        app_event_tx.send(AppEvent::OpenFeedbackConsent { category });
    });
    super::SelectionItem {
        name: name.to_string(),
        description: Some(description.to_string()),
        actions: vec![action],
        dismiss_on_select: true,
        ..Default::default()
    }
}

/// Build the upload consent popup params for a given feedback category.
pub(crate) fn feedback_upload_consent_params(
    app_event_tx: AppEventSender,
    category: FeedbackCategory,
    rollout_path: Option<std::path::PathBuf>,
    auto_review_rollout_filename: Option<String>,
    include_windows_sandbox_log: bool,
    feedback_diagnostics: &FeedbackDiagnostics,
) -> super::SelectionViewParams {
    use super::popup_consts::standard_popup_hint_line;
    let yes_action: super::SelectionAction = Box::new({
        let tx = app_event_tx.clone();
        move |sender: &AppEventSender| {
            let _ = sender;
            tx.send(AppEvent::OpenFeedbackNote {
                category,
                include_logs: true,
            });
        }
    });

    let no_action: super::SelectionAction = Box::new({
        let tx = app_event_tx;
        move |sender: &AppEventSender| {
            let _ = sender;
            tx.send(AppEvent::OpenFeedbackNote {
                category,
                include_logs: false,
            });
        }
    });

    // Build header listing files that would be sent if user consents.
    let mut header_lines: Vec<Box<dyn crate::render::renderable::Renderable>> = vec![
        Line::from("Upload logs?".bold()).into(),
        Line::from("").into(),
        Line::from("The following files will be sent:".dim()).into(),
        Line::from(vec!["  • ".into(), "xdcode-logs.log".into()]).into(),
        Line::from(vec!["  • ".into(), "client-logs.txt (if available)".into()]).into(),
        Line::from(vec![
            "  • ".into(),
            DOCTOR_REPORT_ATTACHMENT_FILENAME.into(),
        ])
        .into(),
        Line::from(vec![
            "  • ".into(),
            format!("{XDCODE_APPS_TOOLS_CACHE_ATTACHMENT_FILENAME} (if available)").into(),
        ])
        .into(),
        Line::from(vec![
            "  • ".into(),
            format!("{XDCODE_APP_DIRECTORY_CACHE_ATTACHMENT_FILENAME} (if available)").into(),
        ])
        .into(),
    ];
    if include_windows_sandbox_log {
        header_lines.push(
            Line::from(vec![
                "  • ".into(),
                WINDOWS_SANDBOX_LOG_ATTACHMENT_FILENAME.into(),
            ])
            .into(),
        );
    }
    if let Some(path) = rollout_path.as_deref()
        && let Some(name) = path.file_name().map(|s| s.to_string_lossy().to_string())
    {
        header_lines.push(Line::from(vec!["  • ".into(), name.into()]).into());
    }
    if let Some(filename) = auto_review_rollout_filename {
        header_lines.push(Line::from(vec!["  • ".into(), filename.into()]).into());
    }
    if !feedback_diagnostics.is_empty() {
        header_lines.push(
            Line::from(vec![
                "  • ".into(),
                FEEDBACK_DIAGNOSTICS_ATTACHMENT_FILENAME.into(),
            ])
            .into(),
        );
    }
    if should_show_feedback_connectivity_details(category, feedback_diagnostics) {
        header_lines.push(Line::from("").into());
        header_lines.push(Line::from("Connectivity diagnostics".bold()).into());
        for diagnostic in feedback_diagnostics.diagnostics() {
            header_lines
                .push(Line::from(vec!["  - ".into(), diagnostic.headline.clone().into()]).into());
            for detail in &diagnostic.details {
                header_lines.push(Line::from(vec!["    - ".dim(), detail.clone().into()]).into());
            }
        }
    }

    super::SelectionViewParams {
        footer_hint: Some(standard_popup_hint_line()),
        items: vec![
            super::SelectionItem {
                name: "Yes".to_string(),
                description: Some(
                    "Share the current Xdcode session logs and diagnostics with the team for troubleshooting"
                        .to_string(),
                ),
                actions: vec![yes_action],
                dismiss_on_select: true,
                ..Default::default()
            },
            super::SelectionItem {
                name: "No".to_string(),
                actions: vec![no_action],
                dismiss_on_select: true,
                ..Default::default()
            },
        ],
        header: Box::new(crate::render::renderable::ColumnRenderable::with(
            header_lines,
        )),
        ..super::SelectionViewParams::confirmation()
    }
}

//! Mode switching remains available when optional discovery fails or omits Default.
use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn incomplete_catalog_allows_leaving_plan_without_changing_model() {
    let (mut chat, _events, _ops) = make_chatwidget_manual(Some("gpt-5.2")).await;
    let plan = collaboration_modes::plan_mask(&chat.model_catalog).unwrap();
    Arc::make_mut(&mut chat.model_catalog).collaboration_modes = vec![plan];
    chat.set_effective_collaboration_mode(CollaborationMode {
        mode: ModeKind::Plan,
        settings: Settings {
            model: "task-model".into(),
            reasoning_effort: Some(ReasoningEffortConfig::High),
            developer_instructions: Some("stale plan prompt".into()),
        },
    });
    chat.cycle_collaboration_mode();
    assert_eq!(
        chat.effective_collaboration_mode(),
        CollaborationMode {
            mode: ModeKind::Default,
            settings: Settings {
                model: "task-model".into(),
                reasoning_effort: Some(ReasoningEffortConfig::High),
                developer_instructions: None
            },
        }
    );
}

#[tokio::test]
async fn unavailable_catalog_preserves_model_and_effort_across_modes() {
    let (mut chat, _events, _ops) = make_chatwidget_manual(Some("gpt-5.2")).await;
    Arc::make_mut(&mut chat.model_catalog)
        .collaboration_modes
        .clear();
    let settings = Settings {
        model: "task-model".into(),
        reasoning_effort: Some(ReasoningEffortConfig::Ultra),
        developer_instructions: None,
    };
    chat.set_effective_collaboration_mode(CollaborationMode {
        mode: ModeKind::Plan,
        settings: settings.clone(),
    });
    chat.cycle_collaboration_mode();
    assert_eq!(
        chat.effective_collaboration_mode(),
        CollaborationMode {
            mode: ModeKind::Default,
            settings: settings.clone()
        }
    );
    chat.cycle_collaboration_mode();
    assert_eq!(
        chat.effective_collaboration_mode(),
        CollaborationMode {
            mode: ModeKind::Plan,
            settings
        }
    );
}

#[tokio::test]
async fn implementation_message_waits_for_mode_acknowledgment() {
    let (mut chat, mut events, mut ops) = make_chatwidget_manual(Some("gpt-5.2")).await;
    chat.thread_id = Some(ThreadId::new());
    let plan = collaboration_modes::plan_mask(&chat.model_catalog).unwrap();
    chat.set_collaboration_mask(plan);
    let mask = collaboration_modes::default_mode_mask(&chat.model_catalog).unwrap();
    chat.submit_user_message_with_mode("Implement the plan.".into(), mask);
    assert_eq!(chat.active_mode_kind(), ModeKind::Plan);
    assert_no_submit_op(&mut ops);
    assert!(std::iter::from_fn(|| events.try_recv().ok()).any(|e| matches!(e,
        AppEvent::SubmitUserMessageWithMode { collaboration_mode, .. } if collaboration_mode.mode == Some(ModeKind::Default))));
    let mode = CollaborationMode {
        mode: ModeKind::Default,
        settings: chat.effective_collaboration_mode().settings,
    };
    chat.set_effective_collaboration_mode(mode);
    chat.submit_confirmed_mode_message("Implement the plan.".into());
    assert_matches!(
        next_submit_op(&mut ops),
        Op::UserTurn {
            collaboration_mode: Some(CollaborationMode {
                mode: ModeKind::Default,
                ..
            }),
            ..
        }
    );
}

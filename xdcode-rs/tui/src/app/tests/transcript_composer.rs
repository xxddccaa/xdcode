//! Regression coverage for transcript viewer input, prompt selection, and restoration.
//!
//! The default-off feature must leave the existing viewer and its draft intact.
//! Disabled hosted analytics leaves the composer and normal history delivery intact.

use super::*;
use crate::pager_overlay::TranscriptHistoryState;
use crate::test_support::test_path_display;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;

pub(super) fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
    buffer
        .content()
        .chunks(usize::from(buffer.area.width))
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn press_key(
    app: &mut App,
    tui: &mut crate::tui::Tui,
    app_server: &mut AppServerSession,
    code: KeyCode,
) -> Result<()> {
    app.handle_tui_event(
        tui,
        app_server,
        TuiEvent::Key(KeyEvent::new(code, KeyModifiers::NONE)),
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn disabled_analytics_preserves_the_draft_without_opening_an_overlay() -> Result<()> {
    let (mut app, _app_event_rx, _op_rx) = make_test_app_with_channels().await;
    let mut app_server = start_config_write_test_app_server(&app).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    app.chat_widget
        .apply_external_edit("preserved draft".into());
    for _ in 0..2 {
        app.handle_event(
            &mut tui,
            &mut app_server,
            AppEvent::OpenAnalytics { view: None },
        )
        .await?;
        assert!(app.overlay.is_none());
        assert!(app.retained_analytics.is_none());
        assert!(!app.backtrack.overlay_preview_active);
        assert_eq!(
            app.chat_widget.composer_text_with_pending(),
            "preserved draft"
        );
    }
    Ok(())
}

#[tokio::test]
async fn disabled_analytics_makes_no_hosted_requests_on_reopen_or_explicit_view() -> Result<()> {
    let (mut app, _app_event_rx, _op_rx) = make_test_app_with_channels().await;
    let http = wiremock::MockServer::start().await;
    // Catch every method and route, including account checks and report loads.
    wiremock::Mock::given(wiremock::matchers::any())
        .respond_with(wiremock::ResponseTemplate::new(/*s*/ 200))
        .expect(/*r*/ 0)
        .mount(&http)
        .await;
    app.config.chatgpt_base_url = format!("{}/backend-api", http.uri());
    app.config.cli_auth_credentials_store_mode = xdcode_login::AuthCredentialsStoreMode::File;
    app_test_support::write_chatgpt_auth(
        &app.config.xdcode_home,
        app_test_support::ChatGptAuthFixture::new("test-access-token")
            .account_id("test-account")
            .chatgpt_account_id("test-account")
            .chatgpt_user_id("test-user")
            .plan_type("plus"),
        xdcode_login::AuthCredentialsStoreMode::File,
    )
    .unwrap();
    let mut app_server = start_config_write_test_app_server(&app).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    app.chat_widget
        .apply_external_edit("preserved draft".into());
    for explicit in [
        None,
        None,
        Some(crate::analytics::TokenActivityView::Weekly),
    ] {
        app.handle_event(
            &mut tui,
            &mut app_server,
            AppEvent::OpenAnalytics { view: explicit },
        )
        .await?;
        app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
            .await?;
        assert!(app.overlay.is_none());
        assert!(app.retained_analytics.is_none());
        assert_eq!(
            app.chat_widget.composer_text_with_pending(),
            "preserved draft"
        );
    }
    assert!(
        http.received_requests()
            .await
            .expect("hosted request recording is enabled")
            .is_empty(),
        "disabled analytics must not contact hosted services, even with account credentials"
    );
    Ok(())
}

#[tokio::test]
async fn disabled_analytics_preserves_queued_and_late_startup_history() -> Result<()> {
    for alternate_screen in [true, false] {
        let (mut app, _app_event_rx, _op_rx) = make_test_app_with_channels().await;
        let mut app_server = start_config_write_test_app_server(&app).await?;
        let mut tui = crate::tui::test_support::make_test_tui()?;
        tui.set_alt_screen_enabled(alternate_screen);
        app.chat_widget
            .apply_external_edit("preserved draft".into());
        app.insert_history_cell(
            &mut tui,
            Box::new(PlainHistoryCell::new(vec!["Startup header".into()])),
        );
        let queued = tui.pending_history_lines_for_test();
        assert!(!queued.is_empty());
        app.handle_event(
            &mut tui,
            &mut app_server,
            AppEvent::OpenAnalytics { view: None },
        )
        .await?;
        assert!(app.overlay.is_none());
        assert_eq!(tui.pending_history_lines_for_test(), queued);
        assert!(app.deferred_history_lines.is_empty());
        app.insert_history_cell(
            &mut tui,
            Box::new(PlainHistoryCell::new(vec![
                "Late startup announcement".into(),
            ])),
        );
        let pending = tui.pending_history_lines_for_test();
        assert!(pending.starts_with(&queued));
        assert!(pending.len() > queued.len());
        assert!(app.deferred_history_lines.is_empty());
        assert_eq!(app.transcript_cells.len(), 2);
        for (cell, expected) in app
            .transcript_cells
            .iter()
            .zip(["Startup header", "Late startup announcement"])
        {
            assert_eq!(lines_to_single_string(&cell.display_lines(100)), expected);
        }
        assert_eq!(
            app.chat_widget.composer_text_with_pending(),
            "preserved draft"
        );
    }
    Ok(())
}

#[tokio::test]
async fn transcript_flag_off_preserves_viewer_and_backtracking() -> Result<()> {
    let (mut app, mut app_event_rx, _op_rx) = make_test_app_with_channels().await;
    let keymap_config = toml::from_str("[composer]\nsubmit = [\"ctrl-x enter\"]")?;
    app.keymap =
        crate::keymap::RuntimeKeymap::from_config(&keymap_config).expect("valid composer chord");
    app.chat_widget
        .apply_keymap_update(keymap_config, &app.keymap);
    let mut app_server = start_config_write_test_app_server(&app).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let session = test_thread_session(ThreadId::new(), app.config.cwd.to_path_buf());
    app.chat_widget.handle_thread_session(session);
    app.transcript_cells = ["first", "second"]
        .map(|message| {
            Arc::new(UserHistoryCell {
                message: message.into(),
                text_elements: Vec::new(),
                local_image_paths: Vec::new(),
                remote_image_urls: Vec::new(),
                presentation: Default::default(),
            }) as Arc<dyn HistoryCell>
        })
        .to_vec();
    app.chat_widget
        .apply_external_edit("preserved draft".into());
    app.open_transcript_overlay(&mut tui);
    for event in [
        TuiEvent::Paste("not composer input".into()),
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
    ] {
        app.handle_tui_event(&mut tui, &mut app_server, event)
            .await?;
    }
    assert_eq!(
        app.chat_widget.composer_text_with_pending(),
        "preserved draft"
    );
    let chord_prefix = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
    app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Key(chord_prefix))
        .await?;
    assert!(!app.key_chord_matcher.is_pending());
    assert!(!app.backtrack.overlay_preview_active);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 12,
    );
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    let Some(Overlay::Transcript(overlay)) = &mut app.overlay else {
        panic!("viewer closed")
    };
    overlay.render(area, &mut buffer);
    insta::assert_snapshot!("transcript_flag_off_viewer", buffer_text(&buffer));
    for (key, selected) in [
        (KeyCode::Esc, 1),
        (KeyCode::Left, 0),
        (KeyCode::Right, 1),
        (KeyCode::Right, 1),
    ] {
        if let Some(Overlay::Transcript(overlay)) = app.overlay.as_mut() {
            overlay.set_history_state(TranscriptHistoryState::LoadingBeginning);
            overlay.render(area, &mut buffer);
            assert_eq!(
                overlay.set_history_state(TranscriptHistoryState::LoadingBeginning),
                TranscriptHistoryState::LoadingBeginning,
            );
        }
        press_key(&mut app, &mut tui, &mut app_server, key).await?;
        let Some(Overlay::Transcript(overlay)) = app.overlay.as_mut() else {
            panic!("viewer closed")
        };
        assert_eq!(
            overlay.set_history_state(TranscriptHistoryState::Complete),
            TranscriptHistoryState::LoadingOlder,
        );
        assert_eq!(app.backtrack.nth_user_message, selected);
    }
    press_key(&mut app, &mut tui, &mut app_server, KeyCode::Enter).await?;
    assert!(app.overlay.is_none());
    assert!(
        std::iter::from_fn(|| app_event_rx.try_recv().ok()).any(|event| matches!(
            event,
            AppEvent::RevertSessionForPromptEdit {
                selected_cell,
                ..
            } if Arc::ptr_eq(&selected_cell, &app.transcript_cells[crate::app_backtrack::nth_user_position(&app.transcript_cells, /*nth*/ 1).unwrap()])
        ))
    );
    Ok(())
}

async fn assert_transcript_close_repaints_inline_draft(mut app: Box<App>) -> Result<()> {
    let mut app_server = start_config_write_test_app_server(&app).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    app.chat_widget.insert_str("EDGE-DRAFT-MUST-SURVIVE");
    app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
        .await?;
    let inline_viewport = tui.terminal.viewport_area;
    app.open_transcript_overlay(&mut tui);
    tui.enter_alt_screen()?;
    app.insert_history_cell(
        &mut tui,
        Box::new(PlainHistoryCell::new(vec!["arrived in transcript".into()])),
    );
    let deferred_history = app.deferred_history_lines.clone();
    assert!(!deferred_history.is_empty());
    app.apply_raw_output_mode(&mut tui, /*enabled*/ true, /*notify*/ false);
    app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
        .await?;
    assert_eq!(app.deferred_history_lines, deferred_history);
    assert!(app.transcript_reflow.has_pending_reflow());

    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)),
    )
    .await?;
    assert_eq!(tui.terminal.viewport_area, inline_viewport);
    app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
        .await?;
    assert!(!app.transcript_reflow.has_pending_reflow());
    assert_eq!(
        app.last_rendered_history_tail
            .as_ref()
            .expect("inline history reflowed")
            .lines,
        deferred_history
    );

    insta::assert_snapshot!(
        "transcript_close_restores_inline_draft",
        buffer_text(crate::custom_terminal::test_support::last_rendered_buffer(
            &tui.terminal
        ))
        .replace(&test_path_display("/tmp/project"), "/tmp/project")
    );
    Ok(())
}

#[tokio::test]
async fn transcript_viewer_close_repaints_preserved_inline_draft() -> Result<()> {
    assert_transcript_close_repaints_inline_draft(make_test_app().await).await
}

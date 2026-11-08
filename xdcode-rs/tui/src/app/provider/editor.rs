use super::App;
use super::BackendId;
use super::Draft;
use super::Event;
use super::Field;
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPaneView;
use crate::bottom_pane::CancellationEvent;
use crate::bottom_pane::TextArea;
use crate::bottom_pane::ViewCompletion;
use crate::render::renderable::Renderable;
use crossterm::cursor::SetCursorStyle;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::WidgetRef;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

pub(super) const VIEW_ID: &str = "model_provider_editor";

pub(super) fn view(
    app: &App,
    backend: BackendId,
    draft: Draft,
    error: Option<String>,
) -> Box<dyn BottomPaneView> {
    let inputs = draft
        .fields()
        .into_iter()
        .map(|field| {
            let mut editor = TextArea::new();
            editor.set_text_clearing_elements(draft.value(field));
            editor.set_keymap_bindings(&app.keymap);
            (field, editor)
        })
        .collect();
    Box::new(Editor {
        inputs,
        draft,
        backend,
        tx: app.app_event_tx.clone(),
        selected: 0,
        error,
        completion: None,
        pending: false,
        alive: Arc::new(AtomicBool::new(true)),
    })
}

struct Editor {
    inputs: Vec<(Field, TextArea)>,
    draft: Draft,
    backend: BackendId,
    tx: AppEventSender,
    selected: usize,
    error: Option<String>,
    completion: Option<ViewCompletion>,
    pending: bool,
    alive: Arc<AtomicBool>,
}

impl Drop for Editor {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
    }
}

impl Editor {
    fn save(&mut self) {
        self.draft.changes = self
            .inputs
            .iter()
            .filter_map(|(field, editor)| {
                let value = editor.text().trim().to_string();
                let original = self
                    .draft
                    .provider
                    .as_ref()
                    .and_then(|provider| provider.values.get(field))
                    .map(String::as_str)
                    .unwrap_or_default();
                (value != original).then_some((*field, value))
            })
            .collect();
        if let Err(error) = self.draft.patch() {
            self.error = Some(error);
            return;
        }
        self.pending = true;
        self.tx.send(AppEvent::Provider(Event::Save {
            backend: self.backend,
            draft: self.draft.clone(),
            alive: Arc::clone(&self.alive),
        }));
    }
    fn input_area(area: Rect, index: usize) -> Rect {
        Rect::new(
            area.x.saturating_add(2),
            area.y.saturating_add(3 + index as u16 * 3),
            area.width.saturating_sub(4),
            2,
        )
    }
}

impl Renderable for Editor {
    fn render(&self, area: Rect, buffer: &mut Buffer) {
        let title = self
            .draft
            .provider
            .as_ref()
            .map(|provider| format!("Provider: {} ({})", provider.name, provider.id))
            .unwrap_or_else(|| "Add Responses provider".into());
        Paragraph::new(title.bold()).render(Rect::new(area.x, area.y, area.width, 1), buffer);
        for (index, (field, editor)) in self.inputs.iter().enumerate() {
            let input = Self::input_area(area, index);
            if input.y >= area.bottom() {
                break;
            }
            let label = format!(
                "{}{}",
                if self.selected == index { "› " } else { "  " },
                field.label()
            );
            Paragraph::new(label.bold()).render(
                Rect::new(area.x, input.y.saturating_sub(1), area.width, 1),
                buffer,
            );
            (&editor).render_ref(
                Rect::new(
                    input.x,
                    input.y,
                    input.width,
                    input.height.min(area.bottom().saturating_sub(input.y)),
                ),
                buffer,
            );
        }
        let footer = area.y.saturating_add(2 + self.inputs.len() as u16 * 3);
        if footer < area.bottom() {
            let save = if self.pending {
                "Saving…"
            } else if self.selected == self.inputs.len() {
                "› Save"
            } else {
                "  Save"
            };
            let mut lines = vec![save.into(), "Tab: move · Enter: next/save · Esc: back".dim().into(),
                "Only changed fields are saved; credentials and current model selections are untouched.".dim().into()];
            if let Some(error) = &self.error {
                lines.push(error.clone().red().into());
            }
            Paragraph::new(lines)
                .wrap(ratatui::widgets::Wrap { trim: false })
                .render(
                    Rect::new(
                        area.x,
                        footer,
                        area.width,
                        area.bottom().saturating_sub(footer),
                    ),
                    buffer,
                );
        }
    }
    fn desired_height(&self, _width: u16) -> u16 {
        6 + self.inputs.len() as u16 * 3 + u16::from(self.error.is_some()) * 3
    }
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        let (_, editor) = self.inputs.get(self.selected)?;
        editor
            .cursor_pos(Self::input_area(area, self.selected))
            .filter(|(_, cursor_y)| *cursor_y < area.bottom())
    }
    fn cursor_style(&self, _area: Rect) -> SetCursorStyle {
        SetCursorStyle::BlinkingBar
    }
}

impl BottomPaneView for Editor {
    fn view_id(&self) -> Option<&'static str> {
        Some(VIEW_ID)
    }
    fn handle_key_event(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press || self.pending {
            return;
        }
        match key.code {
            KeyCode::Tab if key.modifiers.is_empty() => {
                self.selected = (self.selected + 1) % (self.inputs.len() + 1)
            }
            KeyCode::BackTab => {
                self.selected = (self.selected + self.inputs.len()) % (self.inputs.len() + 1)
            }
            KeyCode::Enter if key.modifiers.is_empty() => {
                if self.selected == self.inputs.len() {
                    self.save();
                } else {
                    self.selected += 1;
                }
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => self.save(),
            _ => {
                if let Some((_, editor)) = self.inputs.get_mut(self.selected) {
                    editor.input(key);
                }
            }
        }
    }
    fn handle_paste(&mut self, pasted: String) -> bool {
        if self.pending {
            return false;
        }
        if let Some((_, editor)) = self.inputs.get_mut(self.selected) {
            editor.insert_str(&pasted.replace(['\r', '\n'], ""));
            return true;
        }
        false
    }
    fn on_ctrl_c(&mut self) -> CancellationEvent {
        self.completion = Some(ViewCompletion::Cancelled);
        CancellationEvent::Handled
    }
    fn is_complete(&self) -> bool {
        self.completion.is_some()
    }
    fn completion(&self) -> Option<ViewCompletion> {
        self.completion
    }
}

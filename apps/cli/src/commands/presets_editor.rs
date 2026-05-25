use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use musicum_core::sidecar::{ProcessorEntry, ProcessorRef};
use musicum_core::services::preset_service;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use sea_orm::DatabaseConnection;
use std::io;

#[derive(Clone)]
struct ParamRow {
    key: String,
    value: serde_json::Value,
}

#[derive(PartialEq)]
enum Pane {
    Processors,
    Params,
}

struct EditorState {
    preset_slug: String,
    processors: Vec<ProcessorEntry>,
    proc_state: ListState,
    param_state: ListState,
    active_pane: Pane,
    editing: bool,
    edit_buf: String,
    status_msg: Option<String>,
}

impl EditorState {
    fn new(preset_slug: String, processors: Vec<ProcessorEntry>) -> Self {
        let mut proc_state = ListState::default();
        if !processors.is_empty() {
            proc_state.select(Some(0));
        }
        Self {
            preset_slug,
            processors,
            proc_state,
            param_state: ListState::default(),
            active_pane: Pane::Processors,
            editing: false,
            edit_buf: String::new(),
            status_msg: None,
        }
    }

    fn selected_proc_index(&self) -> Option<usize> {
        self.proc_state.selected()
    }

    fn params_for_selected(&self) -> Vec<ParamRow> {
        let idx = match self.selected_proc_index() {
            Some(i) => i,
            None => return vec![],
        };
        let params = match &self.processors[idx] {
            ProcessorEntry::Structural { processor, .. } => &processor.params,
            ProcessorEntry::AudioPlugin { processor, .. } => &processor.params,
        };
        match params.as_object() {
            None => vec![],
            Some(map) => map
                .iter()
                .map(|(k, v)| ParamRow { key: k.clone(), value: v.clone() })
                .collect(),
        }
    }

    fn instance_id_for_selected(&self) -> Option<&str> {
        self.selected_proc_index().map(|i| match &self.processors[i] {
            ProcessorEntry::Structural { id, .. } => id.as_str(),
            ProcessorEntry::AudioPlugin { id, .. } => id.as_str(),
        })
    }

    fn proc_label(entry: &ProcessorEntry) -> String {
        match entry {
            ProcessorEntry::Structural { id, enabled, processor } => {
                let flag = if *enabled { "" } else { " [off]" };
                format!("[structural] {}{flag}  ({})", processor.id, &id[..8])
            }
            ProcessorEntry::AudioPlugin { id, enabled, processor } => {
                let flag = if *enabled { "" } else { " [off]" };
                format!("[audio-plugin] {}{flag}  ({})", processor.id, &id[..8])
            }
        }
    }

    fn move_up(&mut self) {
        match self.active_pane {
            Pane::Processors => {
                if self.processors.is_empty() {
                    return;
                }
                let i = self.proc_state.selected().unwrap_or(0);
                let next = if i == 0 { self.processors.len() - 1 } else { i - 1 };
                self.proc_state.select(Some(next));
                self.param_state.select(None);
            }
            Pane::Params => {
                let params = self.params_for_selected();
                if params.is_empty() {
                    return;
                }
                let i = self.param_state.selected().unwrap_or(0);
                let next = if i == 0 { params.len() - 1 } else { i - 1 };
                self.param_state.select(Some(next));
            }
        }
    }

    fn move_down(&mut self) {
        match self.active_pane {
            Pane::Processors => {
                if self.processors.is_empty() {
                    return;
                }
                let i = self.proc_state.selected().unwrap_or(0);
                let next = (i + 1) % self.processors.len();
                self.proc_state.select(Some(next));
                self.param_state.select(None);
            }
            Pane::Params => {
                let params = self.params_for_selected();
                if params.is_empty() {
                    return;
                }
                let i = self.param_state.selected().unwrap_or(0);
                let next = (i + 1) % params.len();
                self.param_state.select(Some(next));
            }
        }
    }

    fn enter_edit(&mut self) {
        if self.active_pane != Pane::Params {
            return;
        }
        let params = self.params_for_selected();
        if let Some(idx) = self.param_state.selected() {
            if let Some(row) = params.get(idx) {
                self.edit_buf = match &row.value {
                    serde_json::Value::String(s) => s.clone(),
                    v => v.to_string(),
                };
                self.editing = true;
                self.status_msg = None;
            }
        }
    }

    fn parse_value(s: &str) -> serde_json::Value {
        if let Ok(i) = s.parse::<i64>() {
            return serde_json::Value::Number(i.into());
        }
        if let Ok(f) = s.parse::<f64>() {
            if let Some(n) = serde_json::Number::from_f64(f) {
                return serde_json::Value::Number(n);
            }
        }
        serde_json::Value::String(s.to_string())
    }

    fn apply_edit_to_processors(&mut self, key: &str, value: serde_json::Value) {
        if let Some(idx) = self.selected_proc_index() {
            let params = match &mut self.processors[idx] {
                ProcessorEntry::Structural { processor: ProcessorRef { params, .. }, .. } => params,
                ProcessorEntry::AudioPlugin { processor: ProcessorRef { params, .. }, .. } => params,
            };
            if let Some(map) = params.as_object_mut() {
                map.insert(key.to_string(), value);
            }
        }
    }
}

pub async fn run_editor(
    db: &DatabaseConnection,
    library_dir: &str,
    preset_slug: &str,
) -> Result<()> {
    let preset = preset_service::get_preset_by_slug(db, preset_slug).await?;
    let processors: Vec<ProcessorEntry> =
        serde_json::from_str(&preset.processors).unwrap_or_default();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = EditorState::new(preset_slug.to_string(), processors);

    loop {
        terminal.draw(|f| draw(f, &mut state))?;

        if let Event::Key(key) = event::read()? {
            if state.editing {
                match key.code {
                    KeyCode::Esc => {
                        state.editing = false;
                        state.edit_buf.clear();
                    }
                    KeyCode::Enter => {
                        let params = state.params_for_selected();
                        if let Some(idx) = state.param_state.selected() {
                            if let Some(row) = params.get(idx) {
                                let key = row.key.clone();
                                let value = EditorState::parse_value(&state.edit_buf);
                                let instance_id =
                                    state.instance_id_for_selected().unwrap_or("").to_string();
                                let edit_buf = state.edit_buf.clone();

                                state.apply_edit_to_processors(&key, value.clone());

                                match preset_service::set_processor_param(
                                    db,
                                    library_dir,
                                    &state.preset_slug,
                                    &instance_id,
                                    &key,
                                    value,
                                )
                                .await
                                {
                                    Ok(_) => {
                                        state.status_msg =
                                            Some(format!("saved {key} = {edit_buf}"));
                                    }
                                    Err(e) => {
                                        state.status_msg = Some(format!("error: {e}"));
                                    }
                                }
                                state.editing = false;
                                state.edit_buf.clear();
                            }
                        }
                    }
                    KeyCode::Backspace => {
                        state.edit_buf.pop();
                    }
                    KeyCode::Char(c) => {
                        state.edit_buf.push(c);
                    }
                    _ => {}
                }
            } else {
                match (key.code, key.modifiers) {
                    (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                    (KeyCode::Tab, _) => {
                        state.active_pane = match state.active_pane {
                            Pane::Processors => {
                                let params = state.params_for_selected();
                                if !params.is_empty() {
                                    state.param_state.select(Some(0));
                                }
                                Pane::Params
                            }
                            Pane::Params => Pane::Processors,
                        };
                        state.status_msg = None;
                    }
                    (KeyCode::Up, _) | (KeyCode::Char('k'), _) => state.move_up(),
                    (KeyCode::Down, _) | (KeyCode::Char('j'), _) => state.move_down(),
                    (KeyCode::Enter, _) => state.enter_edit(),
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

fn draw(f: &mut Frame, state: &mut EditorState) {
    let area = f.area();

    let outer_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(outer_chunks[0]);

    draw_processors(f, state, panes[0]);
    draw_params(f, state, panes[1]);
    draw_footer(f, state, outer_chunks[1]);
}

fn draw_processors(f: &mut Frame, state: &mut EditorState, area: Rect) {
    let active = state.active_pane == Pane::Processors;
    let border_style = if active {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };
    let block = Block::default()
        .title(format!(" Preset: {} — Processors ", state.preset_slug))
        .borders(Borders::ALL)
        .border_style(border_style);

    let items: Vec<ListItem> = state
        .processors
        .iter()
        .map(|e| ListItem::new(EditorState::proc_label(e)))
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, area, &mut state.proc_state);
}

fn draw_params(f: &mut Frame, state: &mut EditorState, area: Rect) {
    let active = state.active_pane == Pane::Params;
    let border_style = if active {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };

    let params = state.params_for_selected();
    let selected_param_idx = state.param_state.selected();

    if state.editing {
        let inner = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(3)])
            .split(area);

        let items: Vec<ListItem> = params
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let style = if Some(i) == selected_param_idx {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{}: ", row.key), style),
                    Span::styled(row.value.to_string(), style),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(" Parameters ")
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_symbol("▶ ");

        f.render_widget(list, inner[0]);

        let key_label = selected_param_idx
            .and_then(|i| params.get(i))
            .map(|r| r.key.as_str())
            .unwrap_or("");
        let input = Paragraph::new(state.edit_buf.as_str())
            .block(
                Block::default()
                    .title(format!(" Edit: {key_label} "))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green)),
            );
        f.render_widget(input, inner[1]);
    } else {
        let items: Vec<ListItem> = params
            .iter()
            .map(|row| {
                ListItem::new(Line::from(vec![
                    Span::raw(format!("{}: ", row.key)),
                    Span::styled(row.value.to_string(), Style::default().fg(Color::Green)),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(" Parameters ")
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ ");

        f.render_stateful_widget(list, area, &mut state.param_state);
    }
}

fn draw_footer(f: &mut Frame, state: &EditorState, area: Rect) {
    let text = if state.editing {
        " Enter: confirm · Esc: cancel".to_string()
    } else if let Some(msg) = &state.status_msg {
        format!(" ✓ {msg}  |  ↑/↓ k/j: navigate · Tab: switch pane · Enter: edit · q: quit")
    } else {
        " ↑/↓ k/j: navigate · Tab: switch pane · Enter: edit · q: quit".to_string()
    };
    let style = if state.status_msg.is_some() && !state.editing {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let paragraph = Paragraph::new(text).style(style);
    f.render_widget(paragraph, area);
}

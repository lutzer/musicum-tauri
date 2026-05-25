use std::{future::Future, io, pin::Pin};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use musicum_core::sidecar::{ProcessorEntry, ProcessorRef};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use structural_processor_sdk::processor::ParameterDescriptor;
use uuid::Uuid;

pub type SaveResult<'a> = Pin<Box<dyn Future<Output = Result<()>> + 'a>>;
pub type SaveFn<'a> = Box<dyn Fn(Vec<ProcessorEntry>) -> SaveResult<'a> + 'a>;

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

#[derive(PartialEq)]
enum Mode {
    Normal,
    Picking,
    Editing,
}

struct EditorState {
    title: String,
    processors: Vec<ProcessorEntry>,
    available_types: Vec<(String, String)>,
    proc_state: ListState,
    param_state: ListState,
    active_pane: Pane,
    mode: Mode,
    picker_idx: usize,
    edit_buf: String,
    status_msg: Option<String>,
}

impl EditorState {
    fn new(title: String, processors: Vec<ProcessorEntry>) -> Self {
        let registry = structural_processors::registry();
        let mut available_types: Vec<(String, String)> = registry
            .values()
            .map(|e| {
                let d = (e.descriptor)();
                (d.id.to_string(), d.name.to_string())
            })
            .collect();
        available_types.sort_by(|a, b| a.0.cmp(&b.0));

        let mut proc_state = ListState::default();
        if !processors.is_empty() {
            proc_state.select(Some(0));
        }
        Self {
            title,
            processors,
            available_types,
            proc_state,
            param_state: ListState::default(),
            active_pane: Pane::Processors,
            mode: Mode::Normal,
            picker_idx: 0,
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
        if self.mode != Mode::Normal {
            return;
        }
        let params = self.params_for_selected();
        if let Some(idx) = self.param_state.selected() {
            if let Some(row) = params.get(idx) {
                self.edit_buf = match &row.value {
                    serde_json::Value::String(s) => s.clone(),
                    v => v.to_string(),
                };
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

    fn add_processor(&mut self, type_id: &str) {
        let registry = structural_processors::registry();
        let Some(entry) = registry.values().find(|e| (e.descriptor)().id == type_id) else {
            return;
        };
        let descriptor = (entry.descriptor)();
        let mut params = serde_json::Map::new();
        for p in descriptor.parameters {
            let (id, val) = match p {
                ParameterDescriptor::Time { id, default, .. } => (id, serde_json::json!(default)),
                ParameterDescriptor::Int { id, default, .. } => (id, serde_json::json!(default)),
            };
            params.insert(id.to_string(), val);
        }
        let new_entry = ProcessorEntry::Structural {
            id: Uuid::new_v4().to_string(),
            enabled: true,
            processor: ProcessorRef {
                id: type_id.to_string(),
                params: serde_json::Value::Object(params),
            },
        };
        let insert_at = self.proc_state.selected().map(|i| i + 1).unwrap_or(0);
        self.processors.insert(insert_at, new_entry);
        self.proc_state.select(Some(insert_at));
    }

    fn delete_selected(&mut self) {
        let Some(idx) = self.proc_state.selected() else { return };
        if self.processors.is_empty() {
            return;
        }
        self.processors.remove(idx);
        let new_sel = if self.processors.is_empty() {
            None
        } else {
            Some(idx.saturating_sub(if idx >= self.processors.len() { 1 } else { 0 }))
        };
        self.proc_state.select(new_sel);
        self.param_state.select(None);
    }

    fn move_proc_up(&mut self) {
        let Some(idx) = self.proc_state.selected() else { return };
        if idx == 0 {
            return;
        }
        self.processors.swap(idx, idx - 1);
        self.proc_state.select(Some(idx - 1));
    }

    fn move_proc_down(&mut self) {
        let Some(idx) = self.proc_state.selected() else { return };
        if idx + 1 >= self.processors.len() {
            return;
        }
        self.processors.swap(idx, idx + 1);
        self.proc_state.select(Some(idx + 1));
    }

    fn toggle_selected(&mut self) {
        let Some(idx) = self.proc_state.selected() else { return };
        match &mut self.processors[idx] {
            ProcessorEntry::Structural { enabled, .. } => *enabled = !*enabled,
            ProcessorEntry::AudioPlugin { enabled, .. } => *enabled = !*enabled,
        }
    }
}

pub async fn run<'a>(
    title: &str,
    initial_processors: Vec<ProcessorEntry>,
    save: SaveFn<'a>,
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = EditorState::new(title.to_string(), initial_processors);

    let result = run_loop(&mut terminal, &mut state, &save).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    result
}

async fn run_loop<'a, B>(
    terminal: &mut Terminal<B>,
    state: &mut EditorState,
    save: &SaveFn<'a>,
) -> Result<()>
where
    B: ratatui::backend::Backend + std::io::Write,
{
    loop {
        terminal.draw(|f| draw(f, state))?;

        let Event::Key(key) = event::read()? else { continue };

        match state.mode {
            Mode::Picking => match key.code {
                KeyCode::Esc => {
                    state.mode = Mode::Normal;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if state.picker_idx > 0 {
                        state.picker_idx -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if state.picker_idx + 1 < state.available_types.len() {
                        state.picker_idx += 1;
                    }
                }
                KeyCode::Enter => {
                    if let Some((type_id, _)) = state.available_types.get(state.picker_idx).cloned() {
                        state.add_processor(&type_id);
                        state.mode = Mode::Normal;
                        match save(state.processors.clone()).await {
                            Ok(_) => state.status_msg = Some(format!("added {type_id}")),
                            Err(e) => state.status_msg = Some(format!("error: {e}")),
                        }
                    }
                }
                _ => {}
            },

            Mode::Editing => match key.code {
                KeyCode::Esc => {
                    state.mode = Mode::Normal;
                    state.edit_buf.clear();
                }
                KeyCode::Backspace => {
                    state.edit_buf.pop();
                }
                KeyCode::Char(c) => {
                    state.edit_buf.push(c);
                }
                KeyCode::Enter => {
                    let params = state.params_for_selected();
                    if let Some(idx) = state.param_state.selected() {
                        if let Some(row) = params.get(idx) {
                            let key = row.key.clone();
                            let value = EditorState::parse_value(&state.edit_buf);
                            let label = format!("{key} = {}", state.edit_buf);
                            state.apply_edit_to_processors(&key, value);
                            state.mode = Mode::Normal;
                            state.edit_buf.clear();
                            match save(state.processors.clone()).await {
                                Ok(_) => state.status_msg = Some(format!("saved {label}")),
                                Err(e) => state.status_msg = Some(format!("error: {e}")),
                            }
                        }
                    }
                }
                _ => {}
            },

            Mode::Normal => match (key.code, key.modifiers) {
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
                (KeyCode::Up, KeyModifiers::SHIFT) if state.active_pane == Pane::Processors => {
                    state.move_proc_up();
                    match save(state.processors.clone()).await {
                        Ok(_) => state.status_msg = Some("moved up".to_string()),
                        Err(e) => state.status_msg = Some(format!("error: {e}")),
                    }
                }
                (KeyCode::Down, KeyModifiers::SHIFT) if state.active_pane == Pane::Processors => {
                    state.move_proc_down();
                    match save(state.processors.clone()).await {
                        Ok(_) => state.status_msg = Some("moved down".to_string()),
                        Err(e) => state.status_msg = Some(format!("error: {e}")),
                    }
                }
                (KeyCode::Up, _) | (KeyCode::Char('k'), _) => state.move_up(),
                (KeyCode::Down, _) | (KeyCode::Char('j'), _) => state.move_down(),
                (KeyCode::Enter, _) => {
                    if state.active_pane == Pane::Params {
                        state.enter_edit();
                        state.mode = Mode::Editing;
                    }
                }
                (KeyCode::Char('a'), _) if state.active_pane == Pane::Processors => {
                    state.picker_idx = 0;
                    state.mode = Mode::Picking;
                }
                (KeyCode::Char('d'), _) if state.active_pane == Pane::Processors => {
                    state.delete_selected();
                    match save(state.processors.clone()).await {
                        Ok(_) => state.status_msg = Some("deleted processor".to_string()),
                        Err(e) => state.status_msg = Some(format!("error: {e}")),
                    }
                }
                (KeyCode::Char(' '), _) if state.active_pane == Pane::Processors => {
                    state.toggle_selected();
                    match save(state.processors.clone()).await {
                        Ok(_) => state.status_msg = Some("toggled".to_string()),
                        Err(e) => state.status_msg = Some(format!("error: {e}")),
                    }
                }
                _ => {}
            },
        }
    }
    Ok(())
}

fn draw(f: &mut Frame, state: &mut EditorState) {
    let area = f.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(outer[0]);

    draw_processors(f, state, panes[0]);
    draw_params(f, state, panes[1]);
    draw_footer(f, state, outer[1]);

    if state.mode == Mode::Picking {
        draw_picker_overlay(f, state, area);
    }
}

fn draw_processors(f: &mut Frame, state: &mut EditorState, area: Rect) {
    let active = state.active_pane == Pane::Processors;
    let border_style = if active {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };
    let block = Block::default()
        .title(format!(" {} — Processors ", state.title))
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

    if state.mode == Mode::Editing {
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
        let input = Paragraph::new(state.edit_buf.as_str()).block(
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
    let (text, style) = match state.mode {
        Mode::Editing => (
            " Enter: confirm · Esc: cancel".to_string(),
            Style::default().fg(Color::DarkGray),
        ),
        Mode::Picking => (
            " Enter: add · Esc: cancel".to_string(),
            Style::default().fg(Color::DarkGray),
        ),
        Mode::Normal => {
            let hint = if state.active_pane == Pane::Processors {
                " a: add  d: delete  Shift+↑/↓: reorder  Space: toggle  Tab: switch  q: quit"
            } else {
                " Enter: edit  Tab: switch pane  q: quit"
            };
            if let Some(msg) = &state.status_msg {
                (
                    format!(" ✓ {msg}  |  {}", hint.trim_start()),
                    Style::default().fg(Color::Green),
                )
            } else {
                (hint.to_string(), Style::default().fg(Color::DarkGray))
            }
        }
    };
    f.render_widget(Paragraph::new(text).style(style), area);
}

fn draw_picker_overlay(f: &mut Frame, state: &EditorState, area: Rect) {
    let popup = centered_rect(50, 60, area);
    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = state
        .available_types
        .iter()
        .enumerate()
        .map(|(i, (id, name))| {
            let style = if i == state.picker_idx {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{name} "), style),
                Span::styled(format!("({id})"), Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" Add Processor ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(list, popup);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vert[1])[1]
}

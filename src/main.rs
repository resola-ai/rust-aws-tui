mod app_state;
mod config;
mod toml_parser;
mod ui;
mod utils;
use anyhow::Result;
use app_state::{
    date_selection::{ActiveColumn, DateSelection},
    function_selection::FunctionSelection,
    log_viewer::LogViewer,
    profile_selection::ProfileSelection,
    AppState, FocusedPanel,
};
use config::Config;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

struct App {
    state: AppState,
    profile_selection: ProfileSelection,
    function_selection: Option<FunctionSelection>,
    date_selection: Option<DateSelection>,
    log_viewer: Option<LogViewer>,
    is_loading: bool,
    focused_panel: FocusedPanel,
}

impl App {
    async fn new() -> Result<Self> {
        let config = Config::new()?;
        let profiles = config.aws_profiles;
        Ok(App {
            state: AppState::ProfileSelection,
            profile_selection: ProfileSelection::new(profiles),
            function_selection: None,
            date_selection: None,
            log_viewer: None,
            is_loading: false,
            focused_panel: FocusedPanel::Left,
        })
    }

    async fn select_profile(&mut self) -> Result<()> {
        if let Some(profile) = self.profile_selection.selected_profile() {
            let mut function_selection = FunctionSelection::new(profile);
            function_selection.load_functions().await?;
            self.function_selection = Some(function_selection);
            self.state = AppState::FunctionList;
        }
        Ok(())
    }

    fn enter_date_selection(&mut self) {
        if let Some(function_selection) = &self.function_selection {
            let profile_name = function_selection.profile.name.clone();
            let function_name =
                function_selection.filtered_functions[function_selection.selected_index].clone();

            self.date_selection = Some(DateSelection::new(profile_name, function_name));
            self.state = AppState::DateSelection;
        }
    }

    async fn enter_log_viewer(&mut self) -> Result<()> {
        if let (Some(function_selection), Some(date_selection)) =
            (&self.function_selection, &self.date_selection)
        {
            let function_name =
                function_selection.filtered_functions[function_selection.selected_index].clone();
            let mut log_viewer = LogViewer::new(
                function_name,
                date_selection.from_date,
                date_selection.to_date,
            );

            log_viewer
                .initialize(
                    function_selection.profile.name.clone(),
                    function_selection.profile.region.clone(),
                )
                .await?;

            self.log_viewer = Some(log_viewer);
            self.state = AppState::LogViewer;
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = App::new().await?;

    // Main loop
    loop {
        terminal.draw(|f| match app.state {
            AppState::ProfileSelection => {
                ui::profile_list_view::draw_profile_selection(f, &mut app.profile_selection)
            }
            AppState::FunctionList => {
                if let Some(ref mut function_selection) = app.function_selection {
                    ui::function_list_view::draw_function_selection(f, function_selection)
                }
            }
            AppState::DateSelection => {
                if let Some(ref mut date_selection) = app.date_selection {
                    ui::date_selection::draw_date_selection_panel(f, date_selection);
                }
            }
            AppState::LogViewer => {
                if let Some(ref mut log_viewer) = app.log_viewer {
                    ui::log_view::draw_log_view(
                        f,
                        app.date_selection.as_ref().unwrap(),
                        Some(log_viewer),
                        false,
                        app.focused_panel,
                    )
                }
            }
        })?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match app.state {
                    AppState::ProfileSelection => match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Up | KeyCode::Char('k') => app.profile_selection.previous(),
                        KeyCode::Down | KeyCode::Char('j') => app.profile_selection.next(),
                        KeyCode::Enter => {
                            app.select_profile().await?;
                        }
                        _ => {}
                    },
                    AppState::FunctionList => {
                        if let Some(ref mut function_selection) = app.function_selection {
                            match key.code {
                                KeyCode::Char('q') => break,
                                KeyCode::Esc => {
                                    app.state = AppState::ProfileSelection;
                                    app.function_selection = None;
                                }
                                KeyCode::Enter => {
                                    app.enter_date_selection();
                                }
                                KeyCode::Up => function_selection.previous(),
                                KeyCode::Down => function_selection.next(),
                                KeyCode::Char(c) => {
                                    function_selection.filter_input.push(c);
                                    function_selection.update_filter().await?;
                                }
                                KeyCode::Backspace => {
                                    function_selection.filter_input.pop();
                                    function_selection.update_filter().await?;
                                }
                                KeyCode::PageUp => {
                                    for _ in 0..10 {
                                        function_selection.previous();
                                    }
                                }
                                KeyCode::PageDown => {
                                    for _ in 0..10 {
                                        function_selection.next();
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    AppState::DateSelection => {
                        if let Some(ref mut date_selection) = app.date_selection {
                            match key.code {
                                KeyCode::Char('q') => break,
                                KeyCode::Esc => {
                                    app.state = AppState::FunctionList;
                                    app.date_selection = None;
                                }
                                KeyCode::Char('c') => date_selection.toggle_custom(),
                                KeyCode::Tab => {
                                    if date_selection.active_column == ActiveColumn::CustomRange {
                                        date_selection.toggle_selection()
                                    }
                                }
                                KeyCode::Char('1') => {
                                    date_selection.select_column(ActiveColumn::QuickRanges)
                                }
                                KeyCode::Char('2') => {
                                    date_selection.select_column(ActiveColumn::CustomRange)
                                }
                                KeyCode::Left => {
                                    if date_selection.custom_selection {
                                        date_selection.previous_field()
                                    } else {
                                        date_selection.previous_quick_range()
                                    }
                                }
                                KeyCode::Right => {
                                    if date_selection.custom_selection {
                                        date_selection.next_field()
                                    } else {
                                        date_selection.next_quick_range()
                                    }
                                }
                                KeyCode::Up => {
                                    if date_selection.custom_selection {
                                        date_selection.adjust_current_field(true)
                                    } else {
                                        date_selection.previous_quick_range();
                                    }
                                }
                                KeyCode::Down => {
                                    if date_selection.custom_selection {
                                        date_selection.adjust_current_field(false)
                                    } else {
                                        date_selection.next_quick_range();
                                    }
                                }
                                KeyCode::Enter => {
                                    // Handle final selection
                                    app.enter_log_viewer().await?;
                                }
                                _ => {}
                            }
                        }
                    }
                    AppState::LogViewer => {
                        if let Some(ref mut log_viewer) = app.log_viewer {
                            match key.code {
                                KeyCode::Char('q') => break,
                                KeyCode::Esc => {
                                    app.state = AppState::DateSelection;
                                    app.log_viewer = None;
                                }
                                KeyCode::Up => {
                                    if log_viewer.expanded {
                                        log_viewer.scroll_up();
                                    } else {
                                        log_viewer.move_selection(
                                            -1,
                                            terminal.size().unwrap().height as usize - 8,
                                        );
                                    }
                                }
                                KeyCode::Down => {
                                    if log_viewer.expanded {
                                        // Get the content height from the current log message
                                        if let Some(log) = log_viewer.get_selected_log() {
                                            let message = log.message.as_deref().unwrap_or("");
                                            let content_height = message.lines().count();
                                            let visible_height =
                                                terminal.size().unwrap().height as usize - 8;
                                            log_viewer.scroll_down();
                                        }
                                    } else {
                                        log_viewer.move_selection(
                                            1,
                                            terminal.size().unwrap().height as usize - 8,
                                        );
                                    }
                                }
                                KeyCode::Enter => {
                                    log_viewer.toggle_expand();
                                    log_viewer.scroll_position = 0; // Reset scroll position when toggling
                                }
                                KeyCode::Char(c) if !log_viewer.expanded => {
                                    log_viewer.filter_input.push(c);
                                    log_viewer.update_filter();
                                }
                                KeyCode::Backspace if !log_viewer.expanded => {
                                    log_viewer.filter_input.pop();
                                    log_viewer.update_filter();
                                }
                                KeyCode::PageUp => {
                                    if log_viewer.expanded {
                                        log_viewer.scroll_position =
                                            log_viewer.scroll_position.saturating_sub(10);
                                    } else {
                                        log_viewer.page_up();
                                    }
                                }
                                KeyCode::PageDown => {
                                    if log_viewer.expanded {
                                        log_viewer.scroll_position =
                                            log_viewer.scroll_position.saturating_add(10);
                                    } else {
                                        log_viewer.page_down();
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

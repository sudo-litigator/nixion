use std::io::{self, Stdout};
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Tabs, Wrap},
    Frame, Terminal,
};

use crate::app::{ActiveTab, App, InputMode};

pub type AppTerminal = Terminal<CrosstermBackend<Stdout>>;

pub fn setup_terminal() -> Result<AppTerminal> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

pub fn restore_terminal(terminal: &mut AppTerminal) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

pub fn run(terminal: &mut AppTerminal, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|frame| draw(frame, app))?;

        if app.should_quit {
            break;
        }

        if event::poll(Duration::from_millis(200))? {
            let Event::Key(key) = event::read()? else {
                continue;
            };

            if key.kind == KeyEventKind::Press {
                app.handle_key(key);
            }
        }
    }

    Ok(())
}

fn draw(frame: &mut Frame<'_>, app: &mut App) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(5),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let tabs = Tabs::new(
        [
            ActiveTab::Flake,
            ActiveTab::Installed,
            ActiveTab::Search,
            ActiveTab::Generations,
        ]
        .into_iter()
        .map(|tab| Line::from(tab.title()))
        .collect::<Vec<_>>(),
    )
    .select(match app.active_tab {
        ActiveTab::Flake => 0,
        ActiveTab::Installed => 1,
        ActiveTab::Search => 2,
        ActiveTab::Generations => 3,
    })
    .block(Block::default().title("nixion").borders(Borders::ALL))
    .highlight_style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(tabs, layout[0]);

    match app.active_tab {
        ActiveTab::Flake => draw_flake(frame, app, layout[1]),
        ActiveTab::Installed => draw_installed(frame, app, layout[1]),
        ActiveTab::Search => draw_search(frame, app, layout[1]),
        ActiveTab::Generations => draw_generations(frame, app, layout[1]),
    }

    draw_context_panel(frame, app, layout[2]);

    let status = Paragraph::new(app.status.as_str())
        .block(
            Block::default()
                .title(app.help_text())
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(status, layout[3]);

    if app.active_tab == ActiveTab::Search && app.input_mode == InputMode::Search {
        let cursor_x = layout[2].x + app.search_query.chars().count() as u16 + 1;
        let cursor_y = layout[2].y + 1;
        frame.set_cursor_position((cursor_x, cursor_y));
    }

    if app.active_tab == ActiveTab::Generations && app.input_mode == InputMode::GenerationFilter {
        let cursor_x = layout[2].x + app.generation_filter.chars().count() as u16 + 1;
        let cursor_y = layout[2].y + 1;
        frame.set_cursor_position((cursor_x, cursor_y));
    }

    draw_confirmation_dialog(frame, app);
}

fn draw_flake(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    let host_items = app
        .flake_info
        .as_ref()
        .map(|flake| {
            if flake.hosts.is_empty() {
                vec![ListItem::new("No nixosConfigurations found")]
            } else {
                flake
                    .hosts
                    .iter()
                    .map(|host| {
                        let marker = if host.current { " [current]" } else { "" };
                        ListItem::new(format!("{}{}", host.name, marker))
                    })
                    .collect::<Vec<_>>()
            }
        })
        .unwrap_or_else(|| vec![ListItem::new("No flake detected")]);

    let host_list = List::new(host_items)
        .block(Block::default().title("NixOS Hosts").borders(Borders::ALL))
        .highlight_style(Style::default().bg(Color::Blue))
        .highlight_symbol("> ");
    frame.render_stateful_widget(host_list, sections[0], &mut app.flake_hosts_state);

    if let Some(flake) = &app.flake_info {
        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(10), Constraint::Min(5)])
            .split(sections[1]);

        let selected_host = flake
            .hosts
            .get(app.flake_hosts_state.selected().unwrap_or_default())
            .map(|host| host.name.as_str())
            .unwrap_or("none");

        let summary = Paragraph::new(vec![
            Line::from(format!("Path: {}", flake.path.display())),
            Line::from(format!("Description: {}", flake.description)),
            Line::from(format!("URL: {}", flake.url)),
            Line::from(format!("Revision: {}", flake.revision)),
            Line::from(format!("Last Modified: {}", flake.last_modified)),
            Line::from(format!("Inputs: {}", flake.input_count)),
            Line::from(format!("Hosts: {}", flake.hosts.len())),
            Line::from(format!("Selected Host: {}", selected_host)),
        ])
        .block(
            Block::default()
                .title("Flake Overview")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true });
        frame.render_widget(summary, right[0]);

        let config_items = if flake.config_files.is_empty() {
            vec![ListItem::new("No .nix files found")]
        } else {
            flake
                .config_files
                .iter()
                .take(18)
                .map(|path| ListItem::new(path.as_str()))
                .collect::<Vec<_>>()
        };

        let config_list = List::new(config_items).block(
            Block::default()
                .title("Configuration Files")
                .borders(Borders::ALL),
        );
        frame.render_widget(config_list, right[1]);
    } else {
        let missing = Paragraph::new(
            "No flake found. Start nixion inside a flake repository, or provide a NixOS flake in /etc/nixos.",
        )
        .block(Block::default().title("Flake Overview").borders(Borders::ALL))
        .wrap(Wrap { trim: true });
        frame.render_widget(missing, sections[1]);
    }
}

fn draw_installed(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
    let items = if app.installed.is_empty() {
        vec![ListItem::new("No installed packages found")]
    } else {
        app.installed
            .iter()
            .map(|package| {
                let detail = if package.attr_path.is_empty() {
                    package.source.clone()
                } else {
                    format!("{} | {}", package.attr_path, package.source)
                };

                ListItem::new(vec![
                    Line::from(Span::styled(
                        package.name.as_str(),
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                    Line::from(detail),
                ])
            })
            .collect::<Vec<_>>()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title("Installed Packages")
                .borders(Borders::ALL),
        )
        .highlight_style(Style::default().bg(Color::Blue))
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, area, &mut app.installed_state);
}

fn draw_search(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
    let items = if app.search_results.is_empty() {
        vec![ListItem::new("No search results. Press / to search.")]
    } else {
        app.search_results
            .iter()
            .map(|package| {
                ListItem::new(vec![
                    Line::from(Span::styled(
                        package.attr.as_str(),
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                    Line::from(package.description.as_str()),
                ])
            })
            .collect::<Vec<_>>()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title("Search Results")
                .borders(Borders::ALL),
        )
        .highlight_style(Style::default().bg(Color::Blue))
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, area, &mut app.search_state);
}

fn draw_generations(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
    let rollback_target = app.rollback_target_generation();
    let visible_generations = app.visible_generations();
    let title = if app.generation_filter.trim().is_empty() {
        format!("System Generations ({})", app.generations.len())
    } else {
        format!(
            "System Generations ({}/{})",
            app.filtered_generations_count(),
            app.generations.len()
        )
    };
    let items = if visible_generations.is_empty() {
        let message = if app.generations.is_empty() {
            "No system generations found"
        } else {
            "No generations match the current filter"
        };
        vec![ListItem::new(message)]
    } else {
        visible_generations
            .into_iter()
            .map(|generation| {
                let mut markers = Vec::new();
                if generation.running {
                    markers.push("running");
                }
                if generation.booted {
                    markers.push("boot");
                }
                if rollback_target == Some(generation.generation) {
                    markers.push("rollback");
                }
                let marker = if markers.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", markers.join("]["))
                };

                ListItem::new(vec![
                    Line::from(Span::styled(
                        format!("Generation {}{}", generation.generation, marker),
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                    Line::from(format!(
                        "{} | {} | {}",
                        generation.created_at, generation.age, generation.summary
                    )),
                ])
            })
            .collect::<Vec<_>>()
    };

    let list = List::new(items)
        .block(Block::default().title(title).borders(Borders::ALL))
        .highlight_style(Style::default().bg(Color::Blue))
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, area, &mut app.generation_state);
}

fn draw_context_panel(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
    match app.active_tab {
        ActiveTab::Flake => {
            let content = app
                .flake_info
                .as_ref()
                .map(|flake| {
                    let selected_host = flake
                        .hosts
                        .get(app.flake_hosts_state.selected().unwrap_or_default())
                        .map(|host| host.name.as_str())
                        .unwrap_or("none");
                    format!(
                        "Root: {}\nSelected host: {}",
                        flake.path.display(),
                        selected_host
                    )
                })
                .unwrap_or_else(|| String::from("No flake loaded"));
            let widget = Paragraph::new(content)
                .block(
                    Block::default()
                        .title("Flake Context")
                        .borders(Borders::ALL),
                )
                .wrap(Wrap { trim: true });
            frame.render_widget(widget, area);
        }
        ActiveTab::Installed => {
            let content = app
                .installed
                .get(app.installed_state.selected().unwrap_or_default())
                .map(|package| format!("{}\n{}", package.attr_path, package.source))
                .unwrap_or_else(|| String::from("No installed package selected"));
            let widget = Paragraph::new(content)
                .block(
                    Block::default()
                        .title("Selected Package")
                        .borders(Borders::ALL),
                )
                .wrap(Wrap { trim: true });
            frame.render_widget(widget, area);
        }
        ActiveTab::Search => {
            let title = if app.input_mode == InputMode::Search {
                "Search Query (editing)"
            } else {
                "Search Query"
            };
            let widget = Paragraph::new(app.search_query.as_str())
                .block(Block::default().title(title).borders(Borders::ALL));
            frame.render_widget(widget, area);
        }
        ActiveTab::Generations => {
            if app.input_mode == InputMode::GenerationFilter {
                let widget = Paragraph::new(app.generation_filter.as_str()).block(
                    Block::default()
                        .title(format!(
                            "Generation Filter ({}/{})",
                            app.filtered_generations_count(),
                            app.generations.len()
                        ))
                        .borders(Borders::ALL),
                );
                frame.render_widget(widget, area);
                return;
            }

            let rollback_target = app.rollback_target_generation();
            let cleanup = app
                .cleanup_preview(4)
                .map(|(delete_count, keep_generation, preview)| {
                    format!(
                        "Cleanup: keep {keep_generation}, delete {delete_count} old ({preview})"
                    )
                })
                .unwrap_or_else(|| String::from("Cleanup: nothing to delete"));
            let content = app
                .selected_generation()
                .map(|generation| {
                    let mut statuses = Vec::new();
                    if generation.running {
                        statuses.push("running now");
                    }
                    if generation.booted {
                        statuses.push("boot default");
                    }
                    if rollback_target == Some(generation.generation) {
                        statuses.push("rollback target");
                    }
                    if statuses.is_empty() {
                        statuses.push("inactive");
                    }

                    format!(
                        "Generation {}\nStatus: {}\nCreated: {} ({})\nProfile: {}\nFilter: {}\n{}",
                        generation.generation,
                        statuses.join(", "),
                        generation.created_at,
                        generation.age,
                        generation.summary,
                        app.generation_filter_label(),
                        cleanup
                    )
                })
                .unwrap_or_else(|| format!("No generation selected\n{}", cleanup));
            let widget = Paragraph::new(content)
                .block(
                    Block::default()
                        .title("Selected Generation")
                        .borders(Borders::ALL),
                )
                .wrap(Wrap { trim: true });
            frame.render_widget(widget, area);
        }
    }
}

fn draw_confirmation_dialog(frame: &mut Frame<'_>, app: &App) {
    let Some(prompt) = app.confirmation_prompt() else {
        return;
    };
    let title = app.overlay_title().unwrap_or("Confirm Action");

    let area = centered_rect(frame.area(), 60, 30);
    let widget = Paragraph::new(prompt)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .style(Style::default().bg(Color::Black)),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(widget, area);
}

fn centered_rect(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height_percent) / 2),
            Constraint::Percentage(height_percent),
            Constraint::Percentage((100 - height_percent) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_percent) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(vertical[1])[1]
}

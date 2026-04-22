use std::path::PathBuf;

use anyhow::{anyhow, Result};
use ratatui::widgets::ListState;

use crate::nix::{
    FlakeHost, FlakeInfo, Generation, GenerationAction, InstalledPackage, NixClient, RebuildAction,
    SearchPackage,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActiveTab {
    Flake,
    Installed,
    Search,
    Generations,
}

impl ActiveTab {
    pub fn title(self) -> &'static str {
        match self {
            Self::Flake => "Flake",
            Self::Installed => "Installed",
            Self::Search => "Search",
            Self::Generations => "Generations",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Flake => Self::Installed,
            Self::Installed => Self::Search,
            Self::Search => Self::Generations,
            Self::Generations => Self::Flake,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Flake => Self::Generations,
            Self::Installed => Self::Flake,
            Self::Search => Self::Installed,
            Self::Generations => Self::Search,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputMode {
    Normal,
    Search,
    GenerationFilter,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingAction {
    RunGeneration {
        generation: u32,
        action: GenerationAction,
    },
    RollbackDialog {
        generation: u32,
    },
    DeleteGeneration {
        generation: u32,
    },
    DeleteOldGenerations,
}

pub struct App {
    pub active_tab: ActiveTab,
    pub input_mode: InputMode,
    pub search_query: String,
    pub generation_filter: String,
    pub status: String,
    pub flake_info: Option<FlakeInfo>,
    pub installed: Vec<InstalledPackage>,
    pub search_results: Vec<SearchPackage>,
    pub generations: Vec<Generation>,
    pub flake_hosts_state: ListState,
    pub installed_state: ListState,
    pub search_state: ListState,
    pub generation_state: ListState,
    pub should_quit: bool,
    pending_action: Option<PendingAction>,
    client: NixClient,
}

impl App {
    pub fn new(client: NixClient) -> Self {
        let mut flake_hosts_state = ListState::default();
        flake_hosts_state.select(Some(0));

        let mut installed_state = ListState::default();
        installed_state.select(Some(0));

        let mut search_state = ListState::default();
        search_state.select(Some(0));

        let mut generation_state = ListState::default();
        generation_state.select(Some(0));

        Self {
            active_tab: ActiveTab::Flake,
            input_mode: InputMode::Normal,
            search_query: String::new(),
            generation_filter: String::new(),
            status: String::from("Loading flake, packages and generations..."),
            flake_info: None,
            installed: Vec::new(),
            search_results: Vec::new(),
            generations: Vec::new(),
            flake_hosts_state,
            installed_state,
            search_state,
            generation_state,
            should_quit: false,
            pending_action: None,
            client,
        }
    }

    pub fn init(&mut self) -> Result<()> {
        let flake_status = self.refresh_flake().err().map(|error| error.to_string());
        self.refresh_installed()?;
        self.refresh_generations()?;
        self.status = flake_status.unwrap_or_else(|| String::from("Ready"));
        Ok(())
    }

    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        if self.input_mode != InputMode::Normal {
            self.handle_input_mode(key);
            return;
        }

        if self.pending_action.is_some() {
            self.handle_pending_action_input(key);
            return;
        }

        match key.code {
            crossterm::event::KeyCode::Char('q') => self.should_quit = true,
            crossterm::event::KeyCode::Left => self.active_tab = self.active_tab.previous(),
            crossterm::event::KeyCode::Right => self.active_tab = self.active_tab.next(),
            crossterm::event::KeyCode::Char('j') | crossterm::event::KeyCode::Down => {
                self.select_next()
            }
            crossterm::event::KeyCode::Char('k') | crossterm::event::KeyCode::Up => {
                self.select_previous()
            }
            crossterm::event::KeyCode::Char('r') => self.run_action(|app| app.refresh_active_tab()),
            crossterm::event::KeyCode::Char('/') if self.active_tab == ActiveTab::Search => {
                self.input_mode = InputMode::Search;
                self.status = String::from("Search mode: type a package query and press Enter");
            }
            crossterm::event::KeyCode::Char('/') if self.active_tab == ActiveTab::Generations => {
                self.input_mode = InputMode::GenerationFilter;
                self.status =
                    String::from("Generation filter: type to filter by number or profile");
            }
            crossterm::event::KeyCode::Enter if self.active_tab == ActiveTab::Generations => {
                self.run_action(|app| app.open_rollback_dialog())
            }
            crossterm::event::KeyCode::PageDown if self.active_tab == ActiveTab::Generations => {
                self.select_generation_page_down()
            }
            crossterm::event::KeyCode::PageUp if self.active_tab == ActiveTab::Generations => {
                self.select_generation_page_up()
            }
            crossterm::event::KeyCode::Home if self.active_tab == ActiveTab::Generations => {
                self.select_generation_first()
            }
            crossterm::event::KeyCode::End if self.active_tab == ActiveTab::Generations => {
                self.select_generation_last()
            }
            crossterm::event::KeyCode::Char('u') if self.active_tab == ActiveTab::Flake => {
                self.run_action(|app| app.update_flake())
            }
            crossterm::event::KeyCode::Char('c') if self.active_tab == ActiveTab::Flake => {
                self.run_action(|app| app.check_flake())
            }
            crossterm::event::KeyCode::Char('f') if self.active_tab == ActiveTab::Flake => {
                self.run_action(|app| app.format_flake())
            }
            crossterm::event::KeyCode::Char('w') if self.active_tab == ActiveTab::Flake => {
                self.run_action(|app| app.rebuild_selected_host(RebuildAction::Switch))
            }
            crossterm::event::KeyCode::Char('t') if self.active_tab == ActiveTab::Flake => {
                self.run_action(|app| app.rebuild_selected_host(RebuildAction::Test))
            }
            crossterm::event::KeyCode::Char('b') if self.active_tab == ActiveTab::Flake => {
                self.run_action(|app| app.rebuild_selected_host(RebuildAction::Boot))
            }
            crossterm::event::KeyCode::Char('i') if self.active_tab == ActiveTab::Search => {
                self.run_action(|app| app.install_selected())
            }
            crossterm::event::KeyCode::Char('d') if self.active_tab == ActiveTab::Installed => {
                self.run_action(|app| app.remove_selected())
            }
            crossterm::event::KeyCode::Char('s') if self.active_tab == ActiveTab::Generations => {
                self.run_action(|app| app.queue_generation_action(GenerationAction::Switch))
            }
            crossterm::event::KeyCode::Char('t') if self.active_tab == ActiveTab::Generations => {
                self.run_action(|app| app.queue_generation_action(GenerationAction::Test))
            }
            crossterm::event::KeyCode::Char('b') if self.active_tab == ActiveTab::Generations => {
                self.run_action(|app| app.queue_generation_action(GenerationAction::Boot))
            }
            crossterm::event::KeyCode::Char('x') if self.active_tab == ActiveTab::Generations => {
                self.run_action(|app| app.queue_delete_selected_generation())
            }
            crossterm::event::KeyCode::Char('o') if self.active_tab == ActiveTab::Generations => {
                self.run_action(|app| app.queue_delete_old_generations())
            }
            crossterm::event::KeyCode::Char('p') if self.active_tab == ActiveTab::Generations => {
                self.run_action(|app| app.jump_to_rollback_target())
            }
            _ => {}
        }
    }

    pub fn help_text(&self) -> &'static str {
        if let Some(action) = self.pending_action {
            return match action {
                PendingAction::RollbackDialog { .. } => "s switch  t test  b boot  n cancel",
                _ => "y confirm  n cancel",
            };
        }

        if self.input_mode == InputMode::Search {
            return "Type search query  Enter run  Esc cancel";
        }

        if self.input_mode == InputMode::GenerationFilter {
            return "Type filter  Enter apply  Esc close";
        }

        match self.active_tab {
            ActiveTab::Flake => {
                "Left/Right switch tabs  j/k move  u update  c check  f fmt  w switch  t test  b boot  r refresh  q quit"
            }
            ActiveTab::Installed => "Left/Right switch tabs  j/k move  d remove  r refresh  q quit",
            ActiveTab::Search => {
                "Left/Right switch tabs  / search  Enter run  i install  r refresh  q quit"
            }
            ActiveTab::Generations => {
                "Left/Right switch tabs  j/k move  PgUp/PgDn/Home/End navigate  / filter  Enter rollback  p jump-target  s switch  t test  b boot  x delete  o cleanup-old  r refresh  q quit"
            }
        }
    }

    fn handle_input_mode(&mut self, key: crossterm::event::KeyEvent) {
        match self.input_mode {
            InputMode::Normal => {}
            InputMode::Search => self.handle_search_input(key),
            InputMode::GenerationFilter => self.handle_generation_filter_input(key),
        }
    }

    fn handle_search_input(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            crossterm::event::KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.status = String::from("Search cancelled");
            }
            crossterm::event::KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
                self.run_action(|app| app.run_search());
            }
            crossterm::event::KeyCode::Backspace => {
                self.search_query.pop();
            }
            crossterm::event::KeyCode::Char(character) => {
                self.search_query.push(character);
            }
            _ => {}
        }
    }

    fn handle_generation_filter_input(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            crossterm::event::KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.status = self.generation_filter_status();
            }
            crossterm::event::KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
                self.status = self.generation_filter_status();
            }
            crossterm::event::KeyCode::Backspace => {
                self.generation_filter.pop();
                let len = self.visible_generations_len();
                clamp_selection(&mut self.generation_state, len);
                self.status = self.generation_filter_status();
            }
            crossterm::event::KeyCode::Char(character) => {
                self.generation_filter.push(character);
                let len = self.visible_generations_len();
                clamp_selection(&mut self.generation_state, len);
                self.status = self.generation_filter_status();
            }
            _ => {}
        }
    }

    fn handle_pending_action_input(&mut self, key: crossterm::event::KeyEvent) {
        if let Some(PendingAction::RollbackDialog { generation }) = self.pending_action {
            match key.code {
                crossterm::event::KeyCode::Char('s') => self.run_action(|app| {
                    app.queue_generation_action_for(generation, GenerationAction::Switch)
                }),
                crossterm::event::KeyCode::Char('t') => self.run_action(|app| {
                    app.queue_generation_action_for(generation, GenerationAction::Test)
                }),
                crossterm::event::KeyCode::Char('b') => self.run_action(|app| {
                    app.queue_generation_action_for(generation, GenerationAction::Boot)
                }),
                crossterm::event::KeyCode::Char('n') | crossterm::event::KeyCode::Esc => {
                    self.pending_action = None;
                    self.status = String::from("Rollback dialog closed");
                }
                _ => {}
            }
            return;
        }

        match key.code {
            crossterm::event::KeyCode::Char('y') => {
                self.run_action(|app| app.execute_pending_action())
            }
            crossterm::event::KeyCode::Char('n') | crossterm::event::KeyCode::Esc => {
                self.pending_action = None;
                self.status = String::from("Action cancelled");
            }
            _ => {}
        }
    }

    fn refresh_active_tab(&mut self) -> Result<()> {
        match self.active_tab {
            ActiveTab::Flake => self.refresh_flake(),
            ActiveTab::Installed => self.refresh_installed(),
            ActiveTab::Search => self.run_search(),
            ActiveTab::Generations => self.refresh_generations(),
        }
    }

    fn refresh_flake(&mut self) -> Result<()> {
        let flake_info = self.client.discover_flake()?;
        let selected_host = flake_info
            .hosts
            .iter()
            .position(|host| host.current)
            .or(Some(0));

        self.flake_info = Some(flake_info);

        let host_count = self
            .flake_info
            .as_ref()
            .map(|flake| flake.hosts.len())
            .unwrap_or(0);
        clamp_selection(&mut self.flake_hosts_state, host_count);

        if host_count > 0 {
            self.flake_hosts_state.select(Some(
                selected_host.unwrap_or(0).min(host_count.saturating_sub(1)),
            ));
        }

        if let Some(flake) = &self.flake_info {
            self.status = format!(
                "Loaded flake {} with {} host(s)",
                flake.path.display(),
                flake.hosts.len()
            );
        }

        Ok(())
    }

    fn refresh_installed(&mut self) -> Result<()> {
        self.installed = self.client.installed_packages()?;
        clamp_selection(&mut self.installed_state, self.installed.len());
        self.status = format!("Loaded {} installed packages", self.installed.len());
        Ok(())
    }

    fn refresh_generations(&mut self) -> Result<()> {
        self.generations = self.client.list_generations()?;
        let len = self.visible_generations_len();
        clamp_selection(&mut self.generation_state, len);
        self.status = self.generation_filter_status();
        Ok(())
    }

    fn run_search(&mut self) -> Result<()> {
        self.search_results = self.client.search_packages(&self.search_query)?;
        clamp_selection(&mut self.search_state, self.search_results.len());
        self.status = format!(
            "Found {} packages for '{}'",
            self.search_results.len(),
            self.search_query.trim()
        );
        Ok(())
    }

    fn update_flake(&mut self) -> Result<()> {
        let flake_path = self.flake_path()?;
        self.client.flake_update(&flake_path)?;
        self.refresh_flake()?;
        self.status = format!("Updated flake inputs in {}", flake_path.display());
        Ok(())
    }

    fn check_flake(&mut self) -> Result<()> {
        let flake_path = self.flake_path()?;
        self.client.flake_check(&flake_path)?;
        self.status = format!("flake check passed for {}", flake_path.display());
        Ok(())
    }

    fn format_flake(&mut self) -> Result<()> {
        let flake_path = self.flake_path()?;
        self.client.flake_format(&flake_path)?;
        self.refresh_flake()?;
        self.status = format!("Formatted flake in {}", flake_path.display());
        Ok(())
    }

    fn rebuild_selected_host(&mut self, action: RebuildAction) -> Result<()> {
        let flake_path = self.flake_path()?;
        let host = self
            .selected_flake_host()
            .ok_or_else(|| anyhow!("no nixosConfiguration host selected"))?
            .name
            .clone();

        self.client.rebuild_host(&flake_path, &host, action)?;
        self.refresh_generations()?;
        self.status = format!("Ran nixos-rebuild {} for host {}", action.label(), host);
        Ok(())
    }

    fn install_selected(&mut self) -> Result<()> {
        let selected = self
            .selected_search()
            .ok_or_else(|| anyhow!("no search result selected"))?;
        let attr = selected.attr.clone();
        self.client.install_package(&attr)?;
        self.refresh_installed()?;
        self.status = format!("Installed {attr}");
        Ok(())
    }

    fn remove_selected(&mut self) -> Result<()> {
        let selected = self
            .selected_installed()
            .ok_or_else(|| anyhow!("no installed package selected"))?;
        let index = selected.index.clone();
        let name = selected.name.clone();
        self.client.remove_package(&index)?;
        self.refresh_installed()?;
        self.status = format!("Removed {name}");
        Ok(())
    }

    fn queue_generation_action(&mut self, action: GenerationAction) -> Result<()> {
        let selected = self
            .selected_generation()
            .ok_or_else(|| anyhow!("no generation selected"))?;
        self.queue_generation_action_for(selected.generation, action)
    }

    fn queue_generation_action_for(
        &mut self,
        generation: u32,
        action: GenerationAction,
    ) -> Result<()> {
        self.pending_action = Some(PendingAction::RunGeneration { generation, action });
        self.status = format!(
            "Confirm {} for generation {}? Press y to continue or n to cancel",
            action.label(),
            generation
        );
        Ok(())
    }

    fn queue_delete_selected_generation(&mut self) -> Result<()> {
        let selected = self
            .selected_generation()
            .ok_or_else(|| anyhow!("no generation selected"))?;

        if selected.running || selected.booted {
            return Err(anyhow!("refusing to delete the running or boot generation"));
        }

        let generation = selected.generation;

        self.pending_action = Some(PendingAction::DeleteGeneration { generation });
        self.status = format!(
            "Confirm deletion of generation {}? Press y to continue or n to cancel",
            generation
        );
        Ok(())
    }

    fn queue_delete_old_generations(&mut self) -> Result<()> {
        let (delete_count, keep_generation) = self
            .cleanup_old_summary()
            .ok_or_else(|| anyhow!("no old generations available"))?;

        let running = self
            .running_generation()
            .ok_or_else(|| anyhow!("no running generation detected"))?;

        if running.generation != keep_generation {
            return Err(anyhow!(
                "cleanup is disabled while the running generation differs from the boot generation"
            ));
        }

        self.pending_action = Some(PendingAction::DeleteOldGenerations);
        self.status = format!(
            "Confirm deletion of {delete_count} old generations? Boot generation {keep_generation} will be kept"
        );
        Ok(())
    }

    fn jump_to_rollback_target(&mut self) -> Result<()> {
        let index = self
            .rollback_target_index()
            .ok_or_else(|| anyhow!("no rollback target available"))?;
        let generation = self.generations[index].generation;
        self.select_generation_number(generation);
        self.status = format!("Selected rollback target generation {generation}");
        Ok(())
    }

    fn open_rollback_dialog(&mut self) -> Result<()> {
        let target = self
            .rollback_target()
            .ok_or_else(|| anyhow!("no rollback target available"))?;
        let generation = target.generation;
        self.select_generation_number(generation);
        self.pending_action = Some(PendingAction::RollbackDialog { generation });
        self.status = format!("Rollback target generation {generation}: choose s, t, or b");
        Ok(())
    }

    fn execute_pending_action(&mut self) -> Result<()> {
        let action = self
            .pending_action
            .take()
            .ok_or_else(|| anyhow!("no pending action"))?;

        match action {
            PendingAction::RollbackDialog { .. } => {
                return Err(anyhow!(
                    "rollback dialog requires a switch, test, or boot choice"
                ));
            }
            PendingAction::RunGeneration { generation, action } => {
                self.client.activate_generation(generation, action)?;
                self.refresh_generations()?;
                self.status = format!("Ran {} on generation {}", action.label(), generation);
            }
            PendingAction::DeleteGeneration { generation } => {
                self.client.delete_generation(generation)?;
                self.refresh_generations()?;
                self.status = format!("Deleted generation {generation}");
            }
            PendingAction::DeleteOldGenerations => {
                self.client.delete_old_generations()?;
                self.refresh_generations()?;
                self.status = String::from("Deleted old generations");
            }
        }

        Ok(())
    }

    pub fn confirmation_prompt(&self) -> Option<String> {
        self.pending_action.map(|action| match action {
            PendingAction::RollbackDialog { generation } => {
                let target = self
                    .generations
                    .iter()
                    .find(|candidate| candidate.generation == generation);

                match target {
                    Some(target) => format!(
                        "Rollback target: generation {}\nCreated: {} ({})\nProfile: {}\n\nChoose: s = switch now, t = test temporarily, b = boot on next restart.",
                        generation, target.created_at, target.age, target.summary
                    ),
                    None => format!(
                        "Rollback target: generation {}\n\nChoose: s = switch now, t = test temporarily, b = boot on next restart.",
                        generation
                    ),
                }
            }
            PendingAction::RunGeneration { generation, action } => format!(
                "Run '{}' for system generation {}?\n\nThis executes the selected generation with sudo.",
                action.label(),
                generation
            ),
            PendingAction::DeleteGeneration { generation } => format!(
                "Delete system generation {}?\n\nThis cannot be undone from the TUI.",
                generation
            ),
            PendingAction::DeleteOldGenerations => self
                .cleanup_preview(6)
                .map(|(delete_count, keep_generation, preview)| {
                    format!(
                        "Delete {delete_count} old system generations?\nKeep: generation {keep_generation}\nDelete: {preview}\n\nCleanup is only enabled when the running system already matches the boot generation."
                    )
                })
                .unwrap_or_else(|| String::from("Delete old system generations?")),
        })
    }

    pub fn overlay_title(&self) -> Option<&'static str> {
        self.pending_action.map(|action| match action {
            PendingAction::RollbackDialog { .. } => "Rollback Target",
            _ => "Confirm Action",
        })
    }

    pub fn rollback_target_generation(&self) -> Option<u32> {
        self.rollback_target()
            .map(|generation| generation.generation)
    }

    pub fn filtered_generations_count(&self) -> usize {
        self.visible_generations_len()
    }

    pub fn visible_generations(&self) -> Vec<&Generation> {
        self.visible_generation_indices()
            .into_iter()
            .filter_map(|index| self.generations.get(index))
            .collect()
    }

    pub fn generation_filter_label(&self) -> String {
        if self.generation_filter.trim().is_empty() {
            String::from("all generations")
        } else {
            format!("filter: {}", self.generation_filter.trim())
        }
    }

    pub fn cleanup_old_summary(&self) -> Option<(usize, u32)> {
        let boot_generation = self.boot_generation()?.generation;
        let delete_count = self.cleanup_candidates().len();

        (delete_count > 0).then_some((delete_count, boot_generation))
    }

    pub fn cleanup_preview(&self, limit: usize) -> Option<(usize, u32, String)> {
        let (delete_count, keep_generation) = self.cleanup_old_summary()?;
        let preview_generations = self
            .cleanup_candidates()
            .into_iter()
            .map(|generation| generation.generation.to_string())
            .collect::<Vec<_>>();

        let shown = preview_generations
            .iter()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let remainder = preview_generations.len().saturating_sub(shown.len());
        let mut preview = shown.join(", ");
        if remainder > 0 {
            preview.push_str(&format!(" (+{remainder} more)"));
        }

        Some((delete_count, keep_generation, preview))
    }

    fn rollback_target_index(&self) -> Option<usize> {
        let active_index = self
            .generations
            .iter()
            .position(|generation| generation.booted)
            .or_else(|| {
                self.generations
                    .iter()
                    .position(|generation| generation.running)
            })?;

        (active_index + 1 < self.generations.len()).then_some(active_index + 1)
    }

    fn rollback_target(&self) -> Option<&Generation> {
        self.rollback_target_index()
            .and_then(|index| self.generations.get(index))
    }

    fn visible_generation_indices(&self) -> Vec<usize> {
        let filter = self.generation_filter.trim().to_ascii_lowercase();

        self.generations
            .iter()
            .enumerate()
            .filter(|(_, generation)| {
                if filter.is_empty() {
                    return true;
                }

                let summary = generation.summary.to_ascii_lowercase();
                let created_at = generation.created_at.to_ascii_lowercase();
                let age = generation.age.to_ascii_lowercase();

                generation.generation.to_string().contains(&filter)
                    || summary.contains(&filter)
                    || created_at.contains(&filter)
                    || age.contains(&filter)
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn visible_generations_len(&self) -> usize {
        self.visible_generation_indices().len()
    }

    fn cleanup_candidates(&self) -> Vec<&Generation> {
        let Some(boot_generation) = self
            .boot_generation()
            .map(|generation| generation.generation)
        else {
            return Vec::new();
        };

        self.generations
            .iter()
            .filter(|generation| generation.generation != boot_generation)
            .collect()
    }

    fn generation_filter_status(&self) -> String {
        let visible = self.visible_generations_len();
        let total = self.generations.len();

        if self.generation_filter.trim().is_empty() {
            format!("Loaded {total} system generations")
        } else {
            format!(
                "Showing {visible} of {total} system generations for filter '{}'",
                self.generation_filter.trim()
            )
        }
    }

    fn boot_generation(&self) -> Option<&Generation> {
        self.generations.iter().find(|generation| generation.booted)
    }

    fn running_generation(&self) -> Option<&Generation> {
        self.generations
            .iter()
            .find(|generation| generation.running)
    }

    fn select_next(&mut self) {
        match self.active_tab {
            ActiveTab::Flake => {
                let len = self.flake_hosts_len();
                select_next(&mut self.flake_hosts_state, len)
            }
            ActiveTab::Installed => select_next(&mut self.installed_state, self.installed.len()),
            ActiveTab::Search => select_next(&mut self.search_state, self.search_results.len()),
            ActiveTab::Generations => {
                let len = self.visible_generations_len();
                select_next(&mut self.generation_state, len)
            }
        }
    }

    fn select_previous(&mut self) {
        match self.active_tab {
            ActiveTab::Flake => {
                let len = self.flake_hosts_len();
                select_previous(&mut self.flake_hosts_state, len)
            }
            ActiveTab::Installed => {
                select_previous(&mut self.installed_state, self.installed.len())
            }
            ActiveTab::Search => select_previous(&mut self.search_state, self.search_results.len()),
            ActiveTab::Generations => {
                let len = self.visible_generations_len();
                select_previous(&mut self.generation_state, len)
            }
        }
    }

    fn select_generation_page_down(&mut self) {
        let len = self.visible_generations_len();
        select_by_offset(&mut self.generation_state, len, 10);
    }

    fn select_generation_page_up(&mut self) {
        let len = self.visible_generations_len();
        select_by_offset(&mut self.generation_state, len, -10);
    }

    fn select_generation_first(&mut self) {
        if self.visible_generations_len() > 0 {
            self.generation_state.select(Some(0));
        }
    }

    fn select_generation_last(&mut self) {
        let len = self.visible_generations_len();
        if len > 0 {
            self.generation_state.select(Some(len - 1));
        }
    }

    fn select_generation_number(&mut self, generation: u32) {
        let visible = self.visible_generation_indices();
        if let Some(index) = visible
            .iter()
            .position(|&actual_index| self.generations[actual_index].generation == generation)
        {
            self.generation_state.select(Some(index));
        }
    }

    fn flake_path(&self) -> Result<PathBuf> {
        self.flake_info
            .as_ref()
            .map(|flake| flake.path.clone())
            .ok_or_else(|| anyhow!("no flake available"))
    }

    fn flake_hosts_len(&self) -> usize {
        self.flake_info
            .as_ref()
            .map(|flake| flake.hosts.len())
            .unwrap_or(0)
    }

    fn selected_flake_host(&self) -> Option<&FlakeHost> {
        self.flake_info.as_ref().and_then(|flake| {
            flake
                .hosts
                .get(self.flake_hosts_state.selected().unwrap_or_default())
        })
    }

    fn selected_installed(&self) -> Option<&InstalledPackage> {
        self.installed
            .get(self.installed_state.selected().unwrap_or_default())
    }

    fn selected_search(&self) -> Option<&SearchPackage> {
        self.search_results
            .get(self.search_state.selected().unwrap_or_default())
    }

    pub fn selected_generation(&self) -> Option<&Generation> {
        let visible = self.visible_generation_indices();
        let selected = self.generation_state.selected().unwrap_or_default();
        visible
            .get(selected)
            .and_then(|index| self.generations.get(*index))
    }

    fn run_action<F>(&mut self, action: F)
    where
        F: FnOnce(&mut Self) -> Result<()>,
    {
        if let Err(error) = action(self) {
            self.status = error.to_string();
        }
    }
}

fn clamp_selection(state: &mut ListState, len: usize) {
    if len == 0 {
        state.select(None);
        return;
    }

    let next = state.selected().unwrap_or(0).min(len.saturating_sub(1));
    state.select(Some(next));
}

fn select_next(state: &mut ListState, len: usize) {
    if len == 0 {
        state.select(None);
        return;
    }

    let next = match state.selected() {
        Some(index) if index + 1 < len => index + 1,
        _ => 0,
    };
    state.select(Some(next));
}

fn select_previous(state: &mut ListState, len: usize) {
    if len == 0 {
        state.select(None);
        return;
    }

    let previous = match state.selected() {
        Some(0) | None => len - 1,
        Some(index) => index.saturating_sub(1),
    };
    state.select(Some(previous));
}

fn select_by_offset(state: &mut ListState, len: usize, offset: isize) {
    if len == 0 {
        state.select(None);
        return;
    }

    let current = state.selected().unwrap_or(0);
    let next = if offset.is_negative() {
        current.saturating_sub(offset.unsigned_abs())
    } else {
        current
            .saturating_add(offset as usize)
            .min(len.saturating_sub(1))
    };
    state.select(Some(next));
}

#[cfg(test)]
mod tests {
    use ratatui::widgets::ListState;

    use super::App;
    use crate::nix::{Generation, NixClient};

    #[test]
    fn filters_generations_by_query() {
        let mut app = App::new(NixClient::default());
        app.generations = vec![
            generation(28, "nixos-system-workstation", true, true),
            generation(27, "nixos-system-server", false, false),
        ];
        app.generation_filter = String::from("server");

        let visible = app.visible_generations();

        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].generation, 27);
    }

    #[test]
    fn cleanup_preview_keeps_boot_generation() {
        let mut app = App::new(NixClient::default());
        app.generations = vec![
            generation(28, "nixos-system-workstation", true, true),
            generation(27, "nixos-system-rollback", false, false),
            generation(26, "nixos-system-older", false, false),
        ];
        app.generation_state = ListState::default();

        let preview = app.cleanup_preview(5).unwrap();

        assert_eq!(preview.0, 2);
        assert_eq!(preview.1, 28);
        assert_eq!(preview.2, "27, 26");
    }

    fn generation(generation: u32, summary: &str, booted: bool, running: bool) -> Generation {
        Generation {
            generation,
            summary: summary.to_string(),
            created_at: String::from("2026-04-22 08:00"),
            age: String::from("1h ago"),
            booted,
            running,
        }
    }
}

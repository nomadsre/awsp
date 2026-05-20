use crate::aws_config::SsoProfile;
use crate::cache::LoginStatus;
use crate::palette;
use anyhow::{bail, Context, Result};
use crossterm::cursor::{Hide, MoveToColumn, MoveUp, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::style::{
    Attribute, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::terminal::{self, disable_raw_mode, enable_raw_mode, Clear, ClearType};
use crossterm::{execute, queue};
use std::cmp;
use std::io::{self, Stderr, Write};

const MAX_VISIBLE_ROWS: usize = 8;
const NAME_WIDTH: usize = 30;

#[derive(Debug, Clone)]
struct PickerEntry<'a> {
    profile: &'a SsoProfile,
    status: LoginStatus,
    env: &'static str,
    is_current: bool,
    search_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickerMode {
    Normal,
    Filter,
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter(stderr: &mut Stderr) -> Result<Self> {
        enable_raw_mode().context("failed to enable terminal raw mode")?;
        execute!(stderr, Hide).context("failed to enter awsp picker screen")?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), Show, ResetColor);
    }
}

pub fn select_profile(
    profiles: &[SsoProfile],
    statuses: &[LoginStatus],
    current_profile: Option<&str>,
) -> Result<String> {
    if profiles.is_empty() {
        bail!("no complete AWS SSO profiles found");
    }

    let entries = build_entries(profiles, statuses, current_profile);
    let mut state = PickerState::new(entries, current_profile.map(str::to_string));
    state.run()
}

struct PickerState<'a> {
    entries: Vec<PickerEntry<'a>>,
    current_profile: Option<String>,
    filtered: Vec<usize>,
    selected: usize,
    offset: usize,
    filter: String,
    mode: PickerMode,
    rendered_lines: u16,
}

impl<'a> PickerState<'a> {
    fn new(entries: Vec<PickerEntry<'a>>, current_profile: Option<String>) -> Self {
        let mut state = Self {
            entries,
            current_profile,
            filtered: Vec::new(),
            selected: 0,
            offset: 0,
            filter: String::new(),
            mode: PickerMode::Normal,
            rendered_lines: 0,
        };
        state.apply_filter();
        state
    }

    fn run(&mut self) -> Result<String> {
        let mut stderr = io::stderr();
        let _guard = TerminalGuard::enter(&mut stderr)?;

        loop {
            self.render(&mut stderr)?;

            let Event::Key(key) = event::read().context("failed to read terminal input")? else {
                continue;
            };

            match self.handle_key(key) {
                Ok(Some(selection)) => {
                    self.clear_rendered(&mut stderr)?;
                    return Ok(selection);
                }
                Ok(None) => {}
                Err(error) => {
                    self.clear_rendered(&mut stderr)?;
                    return Err(error);
                }
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<Option<String>> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') => bail!("profile selection cancelled"),
                KeyCode::Char('l') => return Ok(None),
                KeyCode::Char('y') => return Ok(None),
                _ => return Ok(None),
            }
        }

        match self.mode {
            PickerMode::Normal => self.handle_normal_key(key),
            PickerMode::Filter => self.handle_filter_key(key),
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> Result<Option<String>> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_up(),
            KeyCode::Down | KeyCode::Char('j') => self.move_down(),
            KeyCode::Home | KeyCode::Char('g') => self.jump_first(),
            KeyCode::End | KeyCode::Char('G') => self.jump_last(),
            KeyCode::Char('/') => self.mode = PickerMode::Filter,
            KeyCode::Esc | KeyCode::Char('q') => bail!("profile selection cancelled"),
            KeyCode::Enter => {
                let Some(entry_index) = self.filtered.get(self.selected).copied() else {
                    bail!("no profiles match the current filter");
                };
                return Ok(Some(self.entries[entry_index].profile.name.clone()));
            }
            KeyCode::Char(value) if !value.is_control() => {
                self.mode = PickerMode::Filter;
                self.filter.push(value);
                self.apply_filter();
            }
            _ => {}
        }

        Ok(None)
    }

    fn handle_filter_key(&mut self, key: KeyEvent) -> Result<Option<String>> {
        match key.code {
            KeyCode::Esc => {
                self.mode = PickerMode::Normal;
                self.filter.clear();
                self.apply_filter();
            }
            KeyCode::Enter => self.mode = PickerMode::Normal,
            KeyCode::Backspace => {
                self.filter.pop();
                self.apply_filter();
            }
            KeyCode::Up => self.move_up(),
            KeyCode::Down => self.move_down(),
            KeyCode::Char(value) if !value.is_control() => {
                self.filter.push(value);
                self.apply_filter();
            }
            _ => {}
        }

        Ok(None)
    }

    fn move_up(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected = if self.selected == 0 {
            self.filtered.len() - 1
        } else {
            self.selected - 1
        };
        self.keep_selection_visible();
    }

    fn move_down(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.filtered.len();
        self.keep_selection_visible();
    }

    fn jump_first(&mut self) {
        self.selected = 0;
        self.keep_selection_visible();
    }

    fn jump_last(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = self.filtered.len() - 1;
            self.keep_selection_visible();
        }
    }

    fn apply_filter(&mut self) {
        let needle = self.filter.trim().to_ascii_lowercase();
        self.filtered = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                if needle.is_empty() || entry.search_text.contains(&needle) {
                    Some(index)
                } else {
                    None
                }
            })
            .collect();

        if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len().saturating_sub(1);
        }
        self.keep_selection_visible();
    }

    fn keep_selection_visible(&mut self) {
        let visible_rows = self.visible_rows();
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + visible_rows {
            self.offset = self.selected.saturating_sub(visible_rows - 1);
        }
    }

    fn visible_rows(&self) -> usize {
        let terminal_height = terminal::size()
            .map(|(_, height)| height as usize)
            .unwrap_or(24);
        let chrome_rows = if self.mode == PickerMode::Filter || !self.filter.is_empty() {
            8
        } else {
            7
        };
        terminal_height
            .saturating_sub(chrome_rows)
            .max(1)
            .clamp(1, MAX_VISIBLE_ROWS)
    }

    fn render(&mut self, stderr: &mut Stderr) -> Result<()> {
        let (width, _) = terminal::size().unwrap_or((100, 24));
        let width = cmp::max(width as usize, 60);
        let visible_rows = self.visible_rows();
        let end = cmp::min(self.filtered.len(), self.offset + visible_rows);
        let mut lines = 0_u16;

        if self.rendered_lines > 0 {
            queue!(
                stderr,
                MoveUp(self.rendered_lines),
                MoveToColumn(0),
                Clear(ClearType::FromCursorDown)
            )?;
        }

        queue!(
            stderr,
            MoveToColumn(0),
            Print("\r\n    "),
            SetForegroundColor(palette::ACCENT),
            SetAttribute(Attribute::Bold),
            Print("▌"),
            ResetColor,
            SetForegroundColor(palette::FG),
            SetAttribute(Attribute::Bold),
            Print("  Choose a profile"),
            SetAttribute(Attribute::Reset),
            ResetColor,
            Print("\r\n")
        )?;
        lines += 2;

        self.render_current(stderr)?;
        lines += 1;

        if self.mode == PickerMode::Filter || !self.filter.is_empty() {
            queue!(
                stderr,
                Print("    "),
                SetForegroundColor(palette::DIM),
                Print("Filter: "),
                SetForegroundColor(palette::ACCENT),
                Print(&self.filter),
                ResetColor,
                Print("\r\n")
            )?;
            lines += 1;
        }

        queue!(stderr, Print("\r\n"))?;
        lines += 1;

        if self.filtered.is_empty() {
            queue!(
                stderr,
                Print("    "),
                SetForegroundColor(palette::DIM),
                Print("No profiles match the current filter"),
                ResetColor,
                Print("\r\n")
            )?;
            lines += 1;
        } else {
            for (visible_index, entry_index) in self.filtered[self.offset..end].iter().enumerate() {
                let selected = self.offset + visible_index == self.selected;
                self.render_row(stderr, &self.entries[*entry_index], selected, width)?;
                lines += 1;
            }
        }

        queue!(stderr, Print("\r\n"))?;
        lines += 1;
        self.render_footer(stderr, visible_rows, width)?;
        lines += 1;
        self.rendered_lines = lines;
        stderr.flush().context("failed to render awsp picker")
    }

    fn clear_rendered(&mut self, stderr: &mut Stderr) -> Result<()> {
        if self.rendered_lines == 0 {
            return Ok(());
        }

        queue!(
            stderr,
            MoveUp(self.rendered_lines),
            MoveToColumn(0),
            Clear(ClearType::FromCursorDown),
            Show,
            ResetColor
        )?;
        stderr.flush().context("failed to clear awsp picker")?;
        self.rendered_lines = 0;
        Ok(())
    }

    fn render_current(&self, stderr: &mut Stderr) -> io::Result<()> {
        queue!(stderr, Print("    "), SetForegroundColor(palette::DIM))?;
        if let Some(current) = &self.current_profile {
            queue!(
                stderr,
                Print("Currently active: "),
                SetForegroundColor(palette::GREEN),
                SetAttribute(Attribute::Bold),
                Print(current),
                SetAttribute(Attribute::Reset),
                ResetColor
            )?;
        } else {
            queue!(stderr, Print("No active profile"), ResetColor)?;
        }
        queue!(stderr, Print("\r\n"))
    }

    fn render_row(
        &self,
        stderr: &mut Stderr,
        entry: &PickerEntry<'_>,
        selected: bool,
        width: usize,
    ) -> io::Result<()> {
        let marker = if selected { "▸" } else { "·" };
        let region = entry.profile.region.label();
        let pill = format!("[{}]", entry.env);
        let prefix_width = 4 + 2 + NAME_WIDTH + 2 + pill.len() + 2;
        let region_width = width.saturating_sub(prefix_width).max(region.len());

        if selected {
            queue!(
                stderr,
                SetBackgroundColor(palette::ROW_SELECTED_BG),
                MoveToColumn(0),
                Clear(ClearType::CurrentLine)
            )?;
        }

        queue!(stderr, Print("    "))?;
        queue!(
            stderr,
            SetForegroundColor(if selected {
                palette::ACCENT
            } else {
                palette::DIM
            }),
            SetAttribute(if selected {
                Attribute::Bold
            } else {
                Attribute::Reset
            }),
            Print(format!("{marker} ")),
            SetAttribute(Attribute::Reset)
        )?;

        queue!(
            stderr,
            SetForegroundColor(if selected {
                palette::FG
            } else {
                palette::MUTED
            }),
            SetAttribute(if selected {
                Attribute::Bold
            } else {
                Attribute::Reset
            }),
            Print(pad_or_truncate(&entry.profile.name, NAME_WIDTH)),
            SetAttribute(Attribute::Reset),
            ResetColor,
            Print("  "),
            SetForegroundColor(palette::env_color(entry.env)),
            SetAttribute(Attribute::Bold),
            Print(pill),
            SetAttribute(Attribute::Reset),
            ResetColor,
            Print("  "),
            SetForegroundColor(palette::DIM),
            Print(format!("{region:>region_width$}")),
            ResetColor
        )?;

        if selected {
            let used_width = prefix_width + region_width;
            if used_width < width {
                queue!(stderr, Print(" ".repeat(width - used_width)))?;
            }
            queue!(stderr, ResetColor)?;
        }
        queue!(stderr, Print("\r\n"))
    }

    fn render_footer(
        &self,
        stderr: &mut Stderr,
        visible_rows: usize,
        width: usize,
    ) -> io::Result<()> {
        let hint = "↑↓ navigate  ·  enter select  ·  / filter  ·  esc cancel";
        queue!(stderr, Print("    "), SetForegroundColor(palette::DIM))?;
        queue!(stderr, Print(hint))?;

        if self.filtered.len() > visible_rows || !self.filter.is_empty() {
            let start = if self.filtered.is_empty() {
                0
            } else {
                self.offset + 1
            };
            let end = cmp::min(self.filtered.len(), self.offset + visible_rows);
            let count = format!("{start}-{end} of {}", self.filtered.len());
            let used = 4 + hint.chars().count();
            if width > used + count.len() {
                queue!(stderr, Print(" ".repeat(width - used - count.len())))?;
            } else {
                queue!(stderr, Print("  "))?;
            }
            queue!(stderr, Print(count))?;
        }

        queue!(stderr, ResetColor, Print("\r\n"))
    }
}

fn build_entries<'a>(
    profiles: &'a [SsoProfile],
    statuses: &[LoginStatus],
    current_profile: Option<&str>,
) -> Vec<PickerEntry<'a>> {
    let mut entries = profiles
        .iter()
        .enumerate()
        .map(|(original_index, profile)| {
            let env = detect_env(&profile.name);
            PickerEntry {
                profile,
                status: statuses
                    .get(original_index)
                    .copied()
                    .unwrap_or(LoginStatus::Unknown),
                env,
                is_current: Some(profile.name.as_str()) == current_profile,
                search_text: format!(
                    "{} {} {} {}",
                    profile.name,
                    profile.role_name,
                    profile.region.label(),
                    env
                )
                .to_ascii_lowercase(),
            }
        })
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| {
        picker_rank(left)
            .cmp(&picker_rank(right))
            .then_with(|| left.profile.name.cmp(&right.profile.name))
    });

    entries
}

fn picker_rank(entry: &PickerEntry<'_>) -> u8 {
    if entry.is_current {
        return 0;
    }

    match entry.status {
        LoginStatus::Valid => 1,
        LoginStatus::Expired => 2,
        LoginStatus::Unknown => 3,
    }
}

pub fn detect_env(profile_name: &str) -> &'static str {
    let name = profile_name.to_ascii_lowercase();
    if name.contains("prod") || name.contains("production") {
        "prod"
    } else if name.contains("stag") || name.contains("staging") || name.contains("preprod") {
        "staging"
    } else if name.contains("dev") || name.contains("sandbox") || name.contains("test") {
        "dev"
    } else if name.contains("personal") {
        "personal"
    } else if name.contains("client") || name.contains("customer") {
        "client"
    } else {
        "dev"
    }
}

fn pad_or_truncate(value: &str, width: usize) -> String {
    let mut output = value.chars().take(width).collect::<String>();
    let length = output.chars().count();
    if length < width {
        output.push_str(&" ".repeat(width - length));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aws_config::RegionDisplay;

    fn profile(name: &str) -> SsoProfile {
        SsoProfile {
            name: name.to_string(),
            sso_session: Some("corp".to_string()),
            sso_start_url: "https://example.awsapps.com/start".to_string(),
            sso_region: "us-east-1".to_string(),
            account_id: "123456789012".to_string(),
            role_name: "Admin".to_string(),
            region: RegionDisplay::Profile("us-east-1".to_string()),
        }
    }

    #[test]
    fn current_profile_is_first_without_filtering_rows() {
        let profiles = vec![profile("dev"), profile("prod"), profile("staging")];
        let statuses = vec![LoginStatus::Valid, LoginStatus::Unknown, LoginStatus::Valid];
        let entries = build_entries(&profiles, &statuses, Some("prod"));

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].profile.name, "prod");
        assert!(entries.iter().any(|entry| entry.profile.name == "dev"));
        assert!(entries.iter().any(|entry| entry.profile.name == "staging"));
    }

    #[test]
    fn derives_environment_from_profile_name() {
        assert_eq!(detect_env("acme-prod-admin"), "prod");
        assert_eq!(detect_env("acme-staging-dev"), "staging");
        assert_eq!(detect_env("acme-dev-sandbox"), "dev");
        assert_eq!(detect_env("personal-playground"), "personal");
        assert_eq!(detect_env("client-northwind"), "client");
        assert_eq!(detect_env("shared-tools"), "dev");
    }

    #[test]
    fn pads_and_truncates_profile_names() {
        assert_eq!(pad_or_truncate("abc", 5), "abc  ");
        assert_eq!(pad_or_truncate("abcdef", 3), "abc");
    }
}

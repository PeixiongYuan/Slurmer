use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame,
};

use crate::slurm::{Job, JobState};
use crate::ui::columns::{JobColumn, SortColumn};
use std::collections::{HashMap, HashSet};

/// Visible row type for grouped rendering
#[derive(Debug, Clone)]
enum VisibleRow {
    /// Group header row; holds the group key and the index of the representative job
    Group { key: String, rep_job_index: usize },
    /// A concrete job row; holds the index into `jobs`
    Job { job_index: usize },
}

/// Struct to manage the jobs list view
pub struct JobsList {
    pub state: TableState,
    pub jobs: Vec<Job>,
    pub selected_jobs: Vec<usize>,
    pub sort_column: usize,
    pub sort_ascending: bool,
    /// Mapping from group key to list of job indices belonging to the group
    group_map: HashMap<String, Vec<usize>>,
    /// Which groups are currently expanded
    expanded_groups: HashSet<String>,
    /// Flattened rows that are actually rendered (group headers and visible jobs)
    visible_rows: Vec<VisibleRow>,
}

impl JobsList {
    pub fn new() -> Self {
        Self {
            state: TableState::default(),
            jobs: Vec::new(),
            selected_jobs: Vec::new(),
            sort_column: 0, // Default sort by job ID
            sort_ascending: true,
            group_map: HashMap::new(),
            expanded_groups: HashSet::new(),
            visible_rows: Vec::new(),
        }
    }

    /// Update the list of jobs
    pub fn update_jobs(&mut self, jobs: Vec<Job>) {
        self.jobs = jobs;
        // Jobs are already sorted by the squeue command

        // Rebuild grouping and visible rows on every update
        self.rebuild_groups_and_rows();

        // Reset selection if out of bounds
        if let Some(selected) = self.state.selected() {
            if selected >= self.visible_rows.len() {
                self.state.select(Some(0));
            }
        } else if !self.jobs.is_empty() {
            self.state.select(Some(0));
        }
    }

    /// Toggle job selection. If a group header is selected, toggle selection of the whole group.
    pub fn toggle_select(&mut self) {
        if let Some(visible_idx) = self.state.selected() {
            match self.visible_rows.get(visible_idx) {
                Some(VisibleRow::Group { key, .. }) => {
                    if let Some(indices) = self.group_map.get(key) {
                        let all_selected = indices.iter().all(|i| self.selected_jobs.contains(i));
                        if all_selected {
                            // Deselect all in group
                            self.selected_jobs.retain(|i| !indices.contains(i));
                        } else {
                            // Select all in group
                            for idx in indices {
                                if !self.selected_jobs.contains(idx) {
                                    self.selected_jobs.push(*idx);
                                }
                            }
                        }
                    }
                }
                Some(VisibleRow::Job { job_index }) => {
                    if self.selected_jobs.contains(job_index) {
                        self.selected_jobs.retain(|&i| i != *job_index);
                    } else {
                        self.selected_jobs.push(*job_index);
                    }
                }
                None => {}
            }
        }
    }

    /// Judge if all jobs are selected
    pub fn all_selected(&self) -> bool {
        self.selected_jobs.len() == self.jobs.len()
    }

    /// Select all jobs
    pub fn select_all(&mut self) {
        self.selected_jobs = (0..self.jobs.len()).collect();
    }

    /// Clear all selections
    pub fn clear_selection(&mut self) {
        self.selected_jobs.clear();
    }

    /// Update sort configuration based on SortColumn settings
    pub fn update_sort(&mut self, columns: &[JobColumn], sort_columns: &[SortColumn]) {
        if let Some(first_sort) = sort_columns.first() {
            // Find the index of the column in the displayed columns list
            let column_index = columns
                .iter()
                .position(|col| {
                    std::mem::discriminant(col) == std::mem::discriminant(&first_sort.column)
                })
                .unwrap_or(0);

            self.sort_column = column_index;
            self.sort_ascending =
                matches!(first_sort.order, crate::ui::columns::SortOrder::Ascending);
            // No need to sort jobs as sorting is handled by squeue
        }
    }

    /// Navigate to next job
    /// Returns true if selection changed, false otherwise
    pub fn next(&mut self) -> bool {
        if self.visible_rows.is_empty() {
            return false;
        }

        let old_selection = self.state.selected();
        let i = match old_selection {
            Some(i) => {
                if i >= self.visible_rows.len().saturating_sub(1) {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
        old_selection != Some(i)
    }

    /// Navigate to previous job
    /// Returns true if selection changed, false otherwise
    pub fn previous(&mut self) -> bool {
        if self.visible_rows.is_empty() {
            return false;
        }

        let old_selection = self.state.selected();
        let i = match old_selection {
            Some(i) => {
                if i == 0 {
                    self.visible_rows.len().saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
        old_selection != Some(i)
    }

    /// Draw the jobs list widget
    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        columns: &[JobColumn],
        sort_columns: &[SortColumn],
    ) {
        // Update sorting if needed based on sort_columns
        if !sort_columns.is_empty() {
            self.update_sort(columns, sort_columns);
        }

        // Check if columns are empty, show warning if so
        if columns.is_empty() {
            let warning = Paragraph::new("No columns selected. Press 'c' to configure columns.")
                .style(Style::default().fg(Color::Yellow))
                .block(Block::default().title("Warning").borders(Borders::ALL));
            frame.render_widget(warning, area);
            return;
        }

        // Create headers based on selected columns
        let headers: Vec<&str> = columns.iter().map(|col| col.title()).collect();

        // Create header cells with appropriate styling
        let header_cells = headers.iter().enumerate().map(|(_i, &h)| {
            // Check if this column is in the sort list
            let is_sort_column = sort_columns.iter().any(|sc| sc.column.title() == h);
            let sort_indicator = if is_sort_column {
                let sort_col = sort_columns
                    .iter()
                    .find(|sc| sc.column.title() == h)
                    .unwrap();
                match sort_col.order {
                    crate::ui::columns::SortOrder::Ascending => " ↑",
                    crate::ui::columns::SortOrder::Descending => " ↓",
                }
            } else {
                ""
            };

            let header_style = if is_sort_column {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            };

            Cell::from(format!("{}{}", h, sort_indicator)).style(header_style)
        });

        let header = Row::new(header_cells)
            .style(Style::default().bg(Color::DarkGray))
            .height(1);

        // Create rows for visible items (groups and jobs)
        let rows = self.visible_rows.iter().map(|vr| {
            let (job_index, group_key) = match vr {
                VisibleRow::Group { key, rep_job_index } => (*rep_job_index, Some(key.clone())),
                VisibleRow::Job { job_index } => (*job_index, None),
            };

            let job = &self.jobs[job_index];
            let is_selected = match vr {
                VisibleRow::Group { key, .. } => self
                    .group_map
                    .get(key)
                    .map(|indices| indices.iter().any(|idx| self.selected_jobs.contains(idx)))
                    .unwrap_or(false),
                VisibleRow::Job { job_index } => self.selected_jobs.contains(job_index),
            };

            let color = match job.state {
                JobState::Pending => Color::Yellow,
                JobState::Running => Color::Green,
                JobState::Completed => Color::Blue,
                JobState::Failed | JobState::Timeout | JobState::NodeFail | JobState::Boot => {
                    Color::Red
                }
                JobState::Cancelled => Color::Magenta,
                _ => Color::White,
            };

            let style = if is_selected {
                Style::default().fg(color).add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(color)
            };

            // Create cells based on selected columns
            let cells: Vec<Cell> = columns
                .iter()
                .map(|col| {
                    let content = match col {
                        JobColumn::Id => {
                            if let Some(key) = &group_key {
                                let count = self
                                    .group_map
                                    .get(key)
                                    .map(|v| v.len())
                                    .unwrap_or(1);
                                let expanded = self.expanded_groups.contains(key.as_str());
                                let marker = if expanded { "[-]" } else { "[+]" };
                                if count > 1 {
                                    format!("{} {} ({} tasks)", key, marker, count)
                                } else {
                                    job.id.clone()
                                }
                            } else {
                                job.id.clone()
                            }
                        }
                        JobColumn::Name => {
                            // Truncate name if too long
                            if job.name.len() > 30 {
                                format!("{}...", &job.name[0..27])
                            } else {
                                job.name.clone()
                            }
                        }
                        JobColumn::User => job.user.clone(),
                        JobColumn::State => job.state.to_string(),
                        JobColumn::Partition => job.partition.clone(),
                        JobColumn::QoS => job.qos.clone(),
                        JobColumn::Nodes => job.nodes.to_string(),
                        JobColumn::Node => job.node.clone().unwrap_or_else(|| "-".to_string()),
                        JobColumn::CPUs => job.cpus.to_string(),
                        JobColumn::Time => job.time.clone(),
                        JobColumn::Memory => job.memory.clone(),
                        JobColumn::Account => {
                            job.account.clone().unwrap_or_else(|| "-".to_string())
                        }
                        JobColumn::Priority => job
                            .priority
                            .map(|p| p.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        JobColumn::WorkDir => {
                            job.work_dir.clone().unwrap_or_else(|| "-".to_string())
                        }
                        JobColumn::SubmitTime => {
                            job.submit_time.clone().unwrap_or_else(|| "-".to_string())
                        }
                        JobColumn::StartTime => {
                            job.start_time.clone().unwrap_or_else(|| "-".to_string())
                        }
                        JobColumn::EndTime => {
                            job.end_time.clone().unwrap_or_else(|| "-".to_string())
                        }
                        JobColumn::PReason => job
                            .pending_reason
                            .clone()
                            .unwrap_or_else(|| "-".to_string()),
                    };
                    Cell::from(content)
                })
                .collect();

            Row::new(cells).style(style).height(1)
        });

        // Calculate total available width
        // let available_width = area.width.saturating_sub(2); // Subtract 2 for borders

        // Get constraints for columns using the default_width method from JobColumn
        let constraints: Vec<Constraint> = columns
            .iter()
            .map(|col| {
                // Keep only minimal overrides; widths mostly use column defaults
                match col {
                    JobColumn::WorkDir => Constraint::Min(20),
                    JobColumn::SubmitTime | JobColumn::StartTime | JobColumn::EndTime => {
                        Constraint::Length(19)
                    }
                    _ => col.default_width(),
                }
            })
            .collect();

        // Create the table
        let job_count = self.jobs.len();
        let title = format!("{} Jobs", job_count);
        let table = Table::new(rows, constraints)
            .header(header)
            .block(Block::default().borders(Borders::ALL).title(title))
            .row_highlight_style(Style::default().add_modifier(Modifier::BOLD))
            .highlight_symbol(" ▶ ");

        // Render the table
        frame.render_stateful_widget(table, area, &mut self.state);
    }

    /// Get the currently selected job, if any
    pub fn selected_job(&self) -> Option<&Job> {
        match self.state.selected() {
            Some(visible_idx) => match self.visible_rows.get(visible_idx) {
                Some(VisibleRow::Group { key, .. }) => {
                    // Return the first job of the group
                    self.group_map
                        .get(key)
                        .and_then(|indices| indices.first())
                        .and_then(|&idx| self.jobs.get(idx))
                }
                Some(VisibleRow::Job { job_index }) => self.jobs.get(*job_index),
                None => None,
            },
            None => None,
        }
    }

    /// Get all selected jobs
    pub fn get_selected_jobs(&self) -> Vec<String> {
        self.selected_jobs
            .iter()
            .filter_map(|&i| self.jobs.get(i))
            .map(|job| job.id.clone())
            .collect()
    }

    /// Toggle expand/collapse for the group under the current selection
    pub fn toggle_group_expand(&mut self) {
        let Some(visible_idx) = self.state.selected() else { return };
        let target_key = match self.visible_rows.get(visible_idx) {
            Some(VisibleRow::Group { key, .. }) => Some(key.clone()),
            Some(VisibleRow::Job { job_index }) => Some(self.compute_group_key(&self.jobs[*job_index])),
            None => None,
        };

        if let Some(ref key) = target_key {
            if self.expanded_groups.contains(key.as_str()) {
                self.expanded_groups.remove(key.as_str());
            } else {
                self.expanded_groups.insert(key.clone());
            }
        }

        // Rebuild visible rows and keep selection on the group header
        let keep_key = target_key.clone();
        self.rebuild_groups_and_rows();
        if let Some(key) = keep_key {
            if let Some(idx) = self
                .visible_rows
                .iter()
                .position(|vr| matches!(vr, VisibleRow::Group { key: k, .. } if *k == key))
            {
                self.state.select(Some(idx));
            }
        }
    }

    /// Rebuild group mapping and visible rows
    fn rebuild_groups_and_rows(&mut self) {
        // First pass: build complete group map
        self.group_map.clear();
        for (idx, job) in self.jobs.iter().enumerate() {
            let key = self.compute_group_key(job);
            self.group_map.entry(key).or_default().push(idx);
        }

        // Second pass: build visible rows in original order
        self.visible_rows.clear();
        let mut group_header_added: HashSet<String> = HashSet::new();
        let mut job_displayed: HashSet<usize> = HashSet::new();

        for (idx, job) in self.jobs.iter().enumerate() {
            if job_displayed.contains(&idx) {
                continue;
            }

            let key = self.compute_group_key(job);
            let members = self.group_map.get(&key).cloned().unwrap_or_default();
            if members.len() <= 1 {
                // Single job: show as a plain job row
                self.visible_rows.push(VisibleRow::Job { job_index: idx });
                job_displayed.insert(idx);
                continue;
            }

            // Multi-member group: add header once
            if !group_header_added.contains(&key) {
                group_header_added.insert(key.clone());
                self.visible_rows.push(VisibleRow::Group {
                    key: key.clone(),
                    rep_job_index: idx,
                });
            }

            // If expanded, append all member rows now
            if self.expanded_groups.contains(key.as_str()) {
                for m in members {
                    if !job_displayed.contains(&m) {
                        self.visible_rows.push(VisibleRow::Job { job_index: m });
                        job_displayed.insert(m);
                    }
                }
            }
        }
    }

    /// Compute the grouping key for a job. For array jobs like "12345_7", returns "12345".
    fn compute_group_key(&self, job: &Job) -> String {
        if let Some(pos) = job.id.find('_') {
            let (prefix, suffix) = job.id.split_at(pos);
            let suffix = &suffix[1..];
            if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
                return prefix.to_string();
            }
        }
        job.id.clone()
    }
}

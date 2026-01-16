//! Search form state for the advanced search modal
//!
//! This module provides the state management for the advanced search form,
//! including input fields, level selection, and field focus tracking.

use std::collections::HashSet;

use crate::search::{LogLevel, SerializableSearchCriteria};

/// The fields in the search form that can receive focus
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FormField {
    #[default]
    StartTime,
    EndTime,
    Content,
    Source,
    LevelSelect,
    SubmitBtn,
}

impl FormField {
    /// Get the next field in tab order
    pub fn next(&self) -> Self {
        match self {
            FormField::StartTime => FormField::EndTime,
            FormField::EndTime => FormField::Content,
            FormField::Content => FormField::Source,
            FormField::Source => FormField::LevelSelect,
            FormField::LevelSelect => FormField::SubmitBtn,
            FormField::SubmitBtn => FormField::StartTime,
        }
    }

    /// Get the previous field in tab order
    pub fn prev(&self) -> Self {
        match self {
            FormField::StartTime => FormField::SubmitBtn,
            FormField::EndTime => FormField::StartTime,
            FormField::Content => FormField::EndTime,
            FormField::Source => FormField::Content,
            FormField::LevelSelect => FormField::Source,
            FormField::SubmitBtn => FormField::LevelSelect,
        }
    }

    /// Get the display label for this field
    /// Reserved for future use (e.g., accessibility or dynamic label rendering)
    #[allow(dead_code)]
    pub fn label(&self) -> &'static str {
        match self {
            FormField::StartTime => "开始时间",
            FormField::EndTime => "结束时间",
            FormField::Content => "内容正则",
            FormField::Source => "来源文件",
            FormField::LevelSelect => "日志级别",
            FormField::SubmitBtn => "搜索",
        }
    }
}

/// Mode for template dialogs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TemplateMode {
    #[default]
    None,
    /// Saving a template (show name input)
    Saving,
    /// Loading a template (show template list)
    Loading,
}

/// State for the advanced search form
#[derive(Debug, Clone, Default)]
pub struct SearchFormState {
    /// Start time input (string for user editing)
    pub start_time_input: String,
    /// End time input (string for user editing)
    pub end_time_input: String,
    /// Content regex pattern input
    pub content_input: String,
    /// Source file filter input
    pub source_input: String,
    /// Selected log levels
    pub selected_levels: HashSet<LogLevel>,
    /// Currently focused field
    pub focused_field: FormField,
    /// Whether the form is currently open/visible
    pub is_open: bool,
    /// Validation error message (if any)
    pub error_message: Option<String>,
    /// Template mode (none, saving, or loading)
    pub template_mode: TemplateMode,
    /// Template name input (for saving)
    pub template_name_input: String,
    /// Available template names (for loading)
    pub template_list: Vec<String>,
    /// Selected template index (for loading)
    pub template_selected: usize,
    /// Status message (success feedback)
    pub status_message: Option<String>,
}

impl SearchFormState {
    /// Create a new empty search form state
    pub fn new() -> Self {
        Self::default()
    }

    /// Open the search form
    pub fn open(&mut self) {
        self.is_open = true;
        self.focused_field = FormField::StartTime;
        self.error_message = None;
        self.status_message = None;
        self.template_mode = TemplateMode::None;
    }

    /// Close the search form
    pub fn close(&mut self) {
        self.is_open = false;
        self.template_mode = TemplateMode::None;
    }

    /// Clear all form inputs
    /// Reserved for future use (e.g., "Clear Form" button)
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.start_time_input.clear();
        self.end_time_input.clear();
        self.content_input.clear();
        self.source_input.clear();
        self.selected_levels.clear();
        self.error_message = None;
        self.status_message = None;
    }

    /// Move focus to the next field
    pub fn next_field(&mut self) {
        self.focused_field = self.focused_field.next();
    }

    /// Move focus to the previous field
    pub fn prev_field(&mut self) {
        self.focused_field = self.focused_field.prev();
    }

    /// Toggle a log level selection
    pub fn toggle_level(&mut self, level: LogLevel) {
        if self.selected_levels.contains(&level) {
            self.selected_levels.remove(&level);
        } else {
            self.selected_levels.insert(level);
        }
    }

    /// Get the currently focused input buffer (mutable)
    pub fn current_input_mut(&mut self) -> Option<&mut String> {
        match self.focused_field {
            FormField::StartTime => Some(&mut self.start_time_input),
            FormField::EndTime => Some(&mut self.end_time_input),
            FormField::Content => Some(&mut self.content_input),
            FormField::Source => Some(&mut self.source_input),
            FormField::LevelSelect | FormField::SubmitBtn => None,
        }
    }

    /// Check if any search criteria is set
    /// Reserved for future use (e.g., enabling/disabling submit button)
    #[allow(dead_code)]
    pub fn has_criteria(&self) -> bool {
        !self.start_time_input.is_empty()
            || !self.end_time_input.is_empty()
            || !self.content_input.is_empty()
            || !self.source_input.is_empty()
            || !self.selected_levels.is_empty()
    }

    /// Set an error message
    pub fn set_error(&mut self, msg: String) {
        self.error_message = Some(msg);
    }

    /// Clear the error message
    /// Reserved for future use (e.g., auto-clear on field focus)
    #[allow(dead_code)]
    pub fn clear_error(&mut self) {
        self.error_message = None;
    }

    /// Start save template mode
    pub fn start_save_template(&mut self) {
        self.template_mode = TemplateMode::Saving;
        self.template_name_input.clear();
        self.error_message = None;
    }

    /// Start load template mode
    pub fn start_load_template(&mut self, template_names: Vec<String>) {
        self.template_mode = TemplateMode::Loading;
        self.template_list = template_names;
        self.template_selected = 0;
        self.error_message = None;
    }

    /// Exit template mode
    pub fn exit_template_mode(&mut self) {
        self.template_mode = TemplateMode::None;
    }

    /// Select next template in list
    pub fn next_template(&mut self) {
        if !self.template_list.is_empty() {
            self.template_selected = (self.template_selected + 1) % self.template_list.len();
        }
    }

    /// Select previous template in list
    pub fn prev_template(&mut self) {
        if !self.template_list.is_empty() {
            self.template_selected = self.template_selected
                .checked_sub(1)
                .unwrap_or(self.template_list.len() - 1);
        }
    }

    /// Get the currently selected template name
    pub fn selected_template_name(&self) -> Option<&String> {
        self.template_list.get(self.template_selected)
    }

    /// Convert form state to serializable criteria
    pub fn to_serializable_criteria(&self) -> SerializableSearchCriteria {
        SerializableSearchCriteria {
            start_time: if self.start_time_input.is_empty() {
                None
            } else {
                Some(self.start_time_input.clone())
            },
            end_time: if self.end_time_input.is_empty() {
                None
            } else {
                Some(self.end_time_input.clone())
            },
            content_regex: if self.content_input.is_empty() {
                None
            } else {
                Some(self.content_input.clone())
            },
            source_file: if self.source_input.is_empty() {
                None
            } else {
                Some(self.source_input.clone())
            },
            levels: self.selected_levels.iter().cloned().collect(),
        }
    }

    /// Load criteria from a serializable criteria into the form
    pub fn load_from_criteria(&mut self, criteria: &SerializableSearchCriteria) {
        self.start_time_input = criteria.start_time.clone().unwrap_or_default();
        self.end_time_input = criteria.end_time.clone().unwrap_or_default();
        self.content_input = criteria.content_regex.clone().unwrap_or_default();
        self.source_input = criteria.source_file.clone().unwrap_or_default();
        self.selected_levels = criteria.levels.iter().cloned().collect();
    }

    /// Set a success status message
    pub fn set_status(&mut self, msg: String) {
        self.status_message = Some(msg);
        self.error_message = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_cycle() {
        let mut field = FormField::StartTime;
        for _ in 0..6 {
            field = field.next();
        }
        assert_eq!(field, FormField::StartTime);
    }

    #[test]
    fn test_toggle_level() {
        let mut form = SearchFormState::new();
        assert!(form.selected_levels.is_empty());

        form.toggle_level(LogLevel::Error);
        assert!(form.selected_levels.contains(&LogLevel::Error));

        form.toggle_level(LogLevel::Error);
        assert!(!form.selected_levels.contains(&LogLevel::Error));
    }

    #[test]
    fn test_current_input_mut() {
        let mut form = SearchFormState::new();
        form.focused_field = FormField::Content;
        
        if let Some(input) = form.current_input_mut() {
            input.push_str("test");
        }
        
        assert_eq!(form.content_input, "test");
    }
}

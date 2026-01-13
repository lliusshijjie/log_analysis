use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const MAX_HISTORY: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CommandType {
    Search,
    Jump,
    AiPrompt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub timestamp: String,
    pub kind: CommandType,
    pub content: String,
}

pub struct HistoryManager {
    pub entries: Vec<HistoryEntry>,
    pub selected: usize,
    file_path: PathBuf,
}

impl HistoryManager {
    pub fn new() -> Self {
        let file_path = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".loginsight")
            .join("history.json");

        let entries = Self::load_from_file(&file_path).unwrap_or_default();

        let selected = if entries.is_empty() { 0 } else { entries.len() - 1 };

        Self {
            entries,
            selected,
            file_path,
        }
    }

    fn load_from_file(path: &PathBuf) -> Option<Vec<HistoryEntry>> {
        let content = fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    pub fn add(&mut self, kind: CommandType, content: String) {
        if content.trim().is_empty() {
            return;
        }

        // Dedup: skip if same as last entry
        if let Some(last) = self.entries.last() {
            if last.kind == kind && last.content == content {
                return;
            }
        }

        let entry = HistoryEntry {
            timestamp: Local::now().format("%Y-%m-%d %H:%M").to_string(),
            kind,
            content,
        };

        self.entries.push(entry);

        // Limit size
        if self.entries.len() > MAX_HISTORY {
            self.entries.remove(0);
        }

        let _ = self.save();
    }

    pub fn delete(&mut self, index: usize) {
        if index < self.entries.len() {
            self.entries.remove(index);
            if self.selected >= self.entries.len() && !self.entries.is_empty() {
                self.selected = self.entries.len() - 1;
            }
            let _ = self.save();
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.selected = 0;
        let _ = fs::remove_file(&self.file_path);
    }

    fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.entries)?;
        fs::write(&self.file_path, json)
    }

    pub fn next(&mut self) {
        if !self.entries.is_empty() {
            self.selected = (self.selected + 1).min(self.entries.len() - 1);
        }
    }

    pub fn previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn selected_entry(&self) -> Option<&HistoryEntry> {
        self.entries.get(self.selected)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for HistoryManager {
    fn default() -> Self {
        Self::new()
    }
}

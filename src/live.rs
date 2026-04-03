use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use regex::Regex;

use crate::models::LogEntry;
use crate::parser::{decode_line, merge_multiline_bytes, parse_line};

pub struct TailState {
    offsets: HashMap<usize, u64>,
    line_counts: HashMap<usize, usize>,
}

impl TailState {
    pub fn new() -> Self {
        Self {
            offsets: HashMap::new(),
            line_counts: HashMap::new(),
        }
    }

    pub fn init_offset(&mut self, source_id: usize, offset: u64) {
        self.offsets.insert(source_id, offset);
    }

    pub fn init_line_count(&mut self, source_id: usize, count: usize) {
        self.line_counts.insert(source_id, count);
    }

    pub fn read_new_lines(
        &mut self,
        path: &PathBuf,
        source_id: usize,
        re: &Regex,
    ) -> Vec<LogEntry> {
        let Ok(mut file) = File::open(path) else {
            return vec![];
        };
        let Ok(metadata) = file.metadata() else {
            return vec![];
        };
        let file_size = metadata.len();
        let offset = self.offsets.get(&source_id).copied().unwrap_or(0);

        if file_size < offset {
            self.offsets.insert(source_id, 0);
            return self.read_new_lines(path, source_id, re);
        }

        if file_size == offset {
            return vec![];
        }

        if file.seek(SeekFrom::Start(offset)).is_err() {
            return vec![];
        }

        let mut buffer = Vec::new();
        if file.read_to_end(&mut buffer).is_err() {
            return vec![];
        }

        self.offsets.insert(source_id, file_size);

        let base_line = self.line_counts.get(&source_id).copied().unwrap_or(0);
        let lines = merge_multiline_bytes(&buffer);
        let result: Vec<LogEntry> = lines
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                parse_line(&decode_line(b), b, re, source_id, base_line + i + 1)
            })
            .collect();
        self.line_counts.insert(source_id, base_line + lines.len());
        result
    }
}

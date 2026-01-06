use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use regex::Regex;

use crate::models::LogEntry;
use crate::parser::{decode_line, merge_multiline_bytes, parse_line};

pub struct TailState {
    offsets: HashMap<usize, u64>,
}

impl TailState {
    pub fn new() -> Self {
        Self { offsets: HashMap::new() }
    }

    pub fn init_offset(&mut self, source_id: usize, offset: u64) {
        self.offsets.insert(source_id, offset);
    }

    pub fn read_new_lines(
        &mut self,
        path: &PathBuf,
        source_id: usize,
        re: &Regex,
        base_line_index: usize,
    ) -> Vec<LogEntry> {
        let Ok(mut file) = File::open(path) else { return vec![]; };
        let Ok(metadata) = file.metadata() else { return vec![]; };
        let file_size = metadata.len();
        let offset = self.offsets.get(&source_id).copied().unwrap_or(0);

        if file_size < offset {
            self.offsets.insert(source_id, 0);
            return self.read_new_lines(path, source_id, re, base_line_index);
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

        let lines = merge_multiline_bytes(&buffer);
        lines.iter().enumerate()
            .filter_map(|(i, b)| parse_line(&decode_line(b), b, re, source_id, base_line_index + i + 1))
            .collect()
    }
}

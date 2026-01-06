use crate::config::FiltersConfig;
use crate::models::{DisplayEntry, LogEntry};

pub fn fold_noise(logs: Vec<LogEntry>, config: &FiltersConfig) -> Vec<DisplayEntry> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < logs.len() {
        let mut folded = false;

        for rule in &config.fold_rules {
            let matches = |e: &LogEntry| -> bool {
                match rule.match_type.as_str() {
                    "source_file" => rule.patterns.iter().any(|p| e.source_file.contains(p)),
                    "content" => rule.patterns.iter().any(|p| e.content.contains(p)),
                    "level" => rule.patterns.iter().any(|p| e.level.contains(p)),
                    _ => false,
                }
            };

            let mut j = i;
            while j < logs.len() && matches(&logs[j]) { j += 1; }
            if j - i >= config.fold_threshold {
                result.push(DisplayEntry::Folded {
                    start_index: i,
                    end_index: j - 1,
                    count: j - i,
                    summary_text: format!("Folded {} {}", j - i, rule.name),
                });
                i = j;
                folded = true;
                break;
            }
        }

        if folded { continue; }

        let mut j = i;
        while j < logs.len() && logs[i].content == logs[j].content { j += 1; }
        if j - i >= 5 {
            result.push(DisplayEntry::Folded {
                start_index: i,
                end_index: j - 1,
                count: j - i,
                summary_text: format!("Folded {} identical", j - i),
            });
            i = j;
            continue;
        }

        result.push(DisplayEntry::Normal(logs[i].clone()));
        i += 1;
    }
    result
}


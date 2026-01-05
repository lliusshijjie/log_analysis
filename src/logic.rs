use crate::models::{DisplayEntry, LogEntry};

pub fn fold_noise(logs: Vec<LogEntry>) -> Vec<DisplayEntry> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < logs.len() {
        let matches_a = |e: &LogEntry| {
            e.level == "Info" && (e.source_file.contains("UsbCtrl") || e.source_file.contains("EnumDevice"))
        };
        let matches_b = |e: &LogEntry| {
            e.content.contains("DestroyThread") || e.content.contains("Terminate")
        };

        let mut j = i;
        while j < logs.len() && matches_a(&logs[j]) { j += 1; }
        if j - i >= 3 {
            result.push(DisplayEntry::Folded {
                start_index: i,
                end_index: j - 1,
                count: j - i,
                summary_text: format!("Folded {} USB polling", j - i),
            });
            i = j;
            continue;
        }

        j = i;
        while j < logs.len() && matches_b(&logs[j]) { j += 1; }
        if j - i >= 3 {
            result.push(DisplayEntry::Folded {
                start_index: i,
                end_index: j - 1,
                count: j - i,
                summary_text: format!("Folded {} thread cleanup", j - i),
            });
            i = j;
            continue;
        }

        j = i;
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

//! Search template storage and management
//!
//! This module handles saving and loading search templates to/from disk.
//! Templates are stored in a JSON file in the user's config directory.

use std::fs;
use std::path::PathBuf;

use crate::search::{SearchTemplate, SerializableSearchCriteria};

/// Get the path to the templates file
fn get_templates_path() -> PathBuf {
    let base = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".loginsight");
    
    // Ensure directory exists
    let _ = fs::create_dir_all(&base);
    
    base.join("templates.json")
}

/// Load all saved templates from disk
pub fn load_templates() -> Vec<SearchTemplate> {
    let path = get_templates_path();
    
    if !path.exists() {
        return Vec::new();
    }
    
    match fs::read_to_string(&path) {
        Ok(content) => {
            serde_json::from_str(&content).unwrap_or_default()
        }
        Err(_) => Vec::new(),
    }
}

/// Save all templates to disk
fn save_templates(templates: &[SearchTemplate]) -> Result<(), String> {
    let path = get_templates_path();
    let content = serde_json::to_string_pretty(templates)
        .map_err(|e| format!("序列化失败: {}", e))?;
    
    fs::write(&path, content)
        .map_err(|e| format!("写入文件失败: {}", e))?;
    
    Ok(())
}

/// Save a new template (or overwrite existing with same name)
pub fn save_template(name: &str, criteria: &SerializableSearchCriteria) -> Result<(), String> {
    let mut templates = load_templates();
    
    // Remove existing template with same name
    templates.retain(|t| t.name != name);
    
    // Add new template
    templates.push(SearchTemplate::new(name.to_string(), criteria.clone()));
    
    save_templates(&templates)
}

/// Delete a template by name
pub fn delete_template(name: &str) -> Result<(), String> {
    let mut templates = load_templates();
    let original_len = templates.len();
    
    templates.retain(|t| t.name != name);
    
    if templates.len() == original_len {
        return Err(format!("模板 '{}' 不存在", name));
    }
    
    save_templates(&templates)
}

/// Get a template by name
pub fn get_template(name: &str) -> Option<SearchTemplate> {
    load_templates().into_iter().find(|t| t.name == name)
}

/// Get template names for display
pub fn get_template_names() -> Vec<String> {
    load_templates().into_iter().map(|t| t.name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::LogLevel;
    
    #[test]
    fn test_serializable_criteria() {
        let criteria = SerializableSearchCriteria {
            start_time: Some("-1h".to_string()),
            end_time: None,
            content_regex: Some("error".to_string()),
            source_file: None,
            levels: vec![LogLevel::Error, LogLevel::Warn],
        };
        
        let json = serde_json::to_string(&criteria).unwrap();
        let parsed: SerializableSearchCriteria = serde_json::from_str(&json).unwrap();
        
        assert_eq!(parsed.start_time, Some("-1h".to_string()));
        assert_eq!(parsed.levels.len(), 2);
    }
    
    #[test]
    fn test_search_template() {
        let criteria = SerializableSearchCriteria {
            start_time: Some("-1h".to_string()),
            ..Default::default()
        };
        
        let template = SearchTemplate::new("test".to_string(), criteria);
        
        let json = serde_json::to_string(&template).unwrap();
        let parsed: SearchTemplate = serde_json::from_str(&json).unwrap();
        
        assert_eq!(parsed.name, "test");
        assert_eq!(parsed.criteria.start_time, Some("-1h".to_string()));
    }
}

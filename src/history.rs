//! Persistent notification history storage.
//!
//! Stores notifications to a JSON file with a configurable buffer size (default 10,000).
//! Uses a ring buffer approach - oldest entries are removed when the limit is reached.

use crate::error::{Error, Result};
use crate::notification::Urgency;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;

/// Default maximum number of notifications to store in history.
pub const DEFAULT_HISTORY_LIMIT: usize = 10_000;

/// A serializable notification entry for history storage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// The notification ID.
    pub id: u32,
    /// Name of the application that sent the notification.
    pub app_name: String,
    /// Summary text.
    pub summary: String,
    /// Body text.
    pub body: String,
    /// Urgency level as string.
    pub urgency: String,
    /// Unix timestamp when the notification was received.
    pub timestamp: u64,
    /// ISO 8601 formatted timestamp for human readability.
    pub datetime: String,
}

impl HistoryEntry {
    /// Creates a new history entry from notification data.
    pub fn new(
        id: u32,
        app_name: String,
        summary: String,
        body: String,
        urgency: &Urgency,
        timestamp: u64,
    ) -> Self {
        let datetime = DateTime::from_timestamp(timestamp as i64, 0)
            .unwrap_or_else(Utc::now)
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string();

        Self {
            id,
            app_name,
            summary,
            body,
            urgency: urgency.to_string(),
            timestamp,
            datetime,
        }
    }
}

/// Persistent notification history manager.
#[derive(Debug)]
pub struct History {
    /// Path to the history file.
    path: PathBuf,
    /// In-memory buffer of history entries.
    entries: VecDeque<HistoryEntry>,
    /// Maximum number of entries to store.
    limit: usize,
}

impl History {
    /// Creates a new history manager, loading existing history from disk.
    pub fn new(limit: usize) -> Result<Self> {
        let path = Self::default_path()?;
        let entries = Self::load_from_path(&path)?;

        log::debug!(
            "loaded {} history entries from {}",
            entries.len(),
            path.display()
        );

        Ok(Self {
            path,
            entries,
            limit,
        })
    }

    /// Returns the default history file path.
    fn default_path() -> Result<PathBuf> {
        let mut path = dirs::data_local_dir()
            .or_else(dirs::data_dir)
            .or_else(dirs::home_dir)
            .ok_or_else(|| Error::Config("could not determine data directory".to_string()))?;

        path.push("runst");
        fs::create_dir_all(&path)?;
        path.push("history.json");
        Ok(path)
    }

    /// Loads history entries from a file path.
    fn load_from_path(path: &PathBuf) -> Result<VecDeque<HistoryEntry>> {
        if !path.exists() {
            return Ok(VecDeque::new());
        }

        let contents = fs::read_to_string(path)?;
        if contents.trim().is_empty() {
            return Ok(VecDeque::new());
        }

        let entries: Vec<HistoryEntry> = serde_json::from_str(&contents)?;
        Ok(VecDeque::from(entries))
    }

    /// Adds a notification to history and persists to disk.
    pub fn add(&mut self, entry: HistoryEntry) -> Result<()> {
        self.entries.push_back(entry);

        // Enforce limit by removing oldest entries
        while self.entries.len() > self.limit {
            self.entries.pop_front();
        }

        self.save()
    }

    /// Saves the current history to disk.
    fn save(&self) -> Result<()> {
        let entries: Vec<&HistoryEntry> = self.entries.iter().collect();
        let json = serde_json::to_string_pretty(&entries)?;
        fs::write(&self.path, json)?;
        log::trace!(
            "saved {} history entries to {}",
            self.entries.len(),
            self.path.display()
        );
        Ok(())
    }

    /// Returns the number of entries in history.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if history is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the most recent N entries (newest first).
    pub fn recent(&self, count: usize) -> Vec<&HistoryEntry> {
        self.entries.iter().rev().take(count).collect()
    }

    /// Returns all entries (oldest first).
    pub fn all(&self) -> Vec<&HistoryEntry> {
        self.entries.iter().collect()
    }

    /// Searches history entries by app name, summary, or body.
    pub fn search(&self, query: &str) -> Vec<&HistoryEntry> {
        let query_lower = query.to_lowercase();
        self.entries
            .iter()
            .filter(|e| {
                e.app_name.to_lowercase().contains(&query_lower)
                    || e.summary.to_lowercase().contains(&query_lower)
                    || e.body.to_lowercase().contains(&query_lower)
            })
            .collect()
    }

    /// Clears all history entries and saves.
    pub fn clear(&mut self) -> Result<()> {
        self.entries.clear();
        self.save()
    }

    /// Returns the path to the history file.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_entry(id: u32, app_name: &str, summary: &str) -> HistoryEntry {
        HistoryEntry::new(
            id,
            app_name.to_string(),
            summary.to_string(),
            "body".to_string(),
            &Urgency::Normal,
            1234567890,
        )
    }

    #[test]
    fn test_history_entry_creation() {
        let entry = create_test_entry(1, "test_app", "Test Summary");
        assert_eq!(entry.id, 1);
        assert_eq!(entry.app_name, "test_app");
        assert_eq!(entry.summary, "Test Summary");
        assert_eq!(entry.urgency, "normal");
    }

    #[test]
    fn test_history_limit_enforcement() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("history.json");

        let mut history = History {
            path,
            entries: VecDeque::new(),
            limit: 3,
        };

        for i in 0..5 {
            history
                .add(create_test_entry(i, "app", &format!("summary {}", i)))
                .unwrap();
        }

        assert_eq!(history.len(), 3);
        // Should have entries 2, 3, 4 (oldest removed)
        let entries: Vec<_> = history.all();
        assert_eq!(entries[0].id, 2);
        assert_eq!(entries[2].id, 4);
    }

    #[test]
    fn test_history_search() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("history.json");

        let mut history = History {
            path,
            entries: VecDeque::new(),
            limit: 100,
        };

        history
            .add(create_test_entry(1, "firefox", "Download complete"))
            .unwrap();
        history
            .add(create_test_entry(2, "slack", "New message"))
            .unwrap();
        history
            .add(create_test_entry(3, "firefox", "Page loaded"))
            .unwrap();

        let results = history.search("firefox");
        assert_eq!(results.len(), 2);

        let results = history.search("message");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].app_name, "slack");
    }

    #[test]
    fn test_history_recent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("history.json");

        let mut history = History {
            path,
            entries: VecDeque::new(),
            limit: 100,
        };

        for i in 0..10 {
            history
                .add(create_test_entry(i, "app", &format!("summary {}", i)))
                .unwrap();
        }

        let recent = history.recent(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].id, 9); // Most recent first
        assert_eq!(recent[1].id, 8);
        assert_eq!(recent[2].id, 7);
    }
}

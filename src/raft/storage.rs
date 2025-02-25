use log::info;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::raft::types::LogEntry;

pub struct Storage {
    log_dir: PathBuf,
    node_id: u64,
    current_file: File,
}

impl Storage {
    pub fn new(node_id: u64) -> io::Result<Self> {
        let log_dir = Path::new("raft_logs").to_path_buf();
        fs::create_dir_all(&log_dir)?;

        let log_file = log_dir.join(format!("node_{}.log", node_id));
        info!(
            "Initializing storage for node {} at {:?}",
            node_id, log_file
        );

        let current_file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&log_file)?;

        Ok(Storage {
            log_dir,
            node_id,
            current_file,
        })
    }

    pub fn append_entry(&mut self, entry: &LogEntry) -> io::Result<()> {
        let serialized = serde_json::to_string(&entry)?;
        writeln!(self.current_file, "{}", serialized)?;
        self.current_file.flush()?;
        Ok(())
    }

    pub fn read_all_entries(&self) -> io::Result<Vec<LogEntry>> {
        let file = File::open(self.log_dir.join(format!("node_{}.log", self.node_id)))?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if let Ok(entry) = serde_json::from_str::<LogEntry>(&line) {
                entries.push(entry);
            }
        }

        Ok(entries)
    }

    pub fn clear(&mut self) -> io::Result<()> {
        // Truncate the file
        self.current_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(self.log_dir.join(format!("node_{}.log", self.node_id)))?;
        Ok(())
    }
}

impl Drop for Storage {
    fn drop(&mut self) {
        if let Err(e) = self.current_file.flush() {
            eprintln!("Error flushing log file: {}", e);
        }
    }
}

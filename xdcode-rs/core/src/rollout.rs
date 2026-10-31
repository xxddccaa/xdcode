use crate::config::Config;
pub use xdcode_rollout::ARCHIVED_SESSIONS_SUBDIR;
pub use xdcode_rollout::Cursor;
pub use xdcode_rollout::INTERACTIVE_SESSION_SOURCES;
pub use xdcode_rollout::RolloutRecorder;
pub use xdcode_rollout::RolloutRecorderParams;
pub use xdcode_rollout::SESSIONS_SUBDIR;
pub use xdcode_rollout::SessionMeta;
pub use xdcode_rollout::SortDirection;
pub use xdcode_rollout::ThreadItem;
pub use xdcode_rollout::ThreadSortKey;
pub use xdcode_rollout::ThreadsPage;
pub use xdcode_rollout::append_thread_name;
pub use xdcode_rollout::find_archived_thread_path_by_id_str;
#[deprecated(note = "use find_thread_path_by_id_str")]
pub use xdcode_rollout::find_conversation_path_by_id_str;
pub use xdcode_rollout::find_thread_meta_by_name_str;
pub use xdcode_rollout::find_thread_name_by_id;
pub use xdcode_rollout::find_thread_names_by_ids;
pub use xdcode_rollout::find_thread_path_by_id_str;
pub use xdcode_rollout::parse_cursor;
pub use xdcode_rollout::read_head_for_summary;
pub use xdcode_rollout::read_session_meta_line;
pub use xdcode_rollout::rollout_date_parts;

impl xdcode_rollout::RolloutConfigView for Config {
    fn xdcode_home(&self) -> &std::path::Path {
        self.xdcode_home.as_path()
    }

    fn sqlite_config(&self) -> &xdcode_state::SqliteConfig {
        self.sqlite_config()
    }

    fn cwd(&self) -> &std::path::Path {
        self.cwd.as_path()
    }

    fn model_provider_id(&self) -> &str {
        self.model_provider_id.as_str()
    }

    fn generate_memories(&self) -> bool {
        self.memories.generate_memories
    }
}

pub(crate) mod list {
    pub use xdcode_rollout::find_thread_path_by_id_str;
}

#[cfg(test)]
pub(crate) mod recorder {
    pub use xdcode_rollout::RolloutRecorder;
}

pub(crate) use crate::session_rollout_init_error::map_session_init_error;

pub(crate) mod truncation {
    pub(crate) use crate::thread_rollout_truncation::*;
}

use crate::utils::id_generator;

use super::CheckpointerType;

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub session_id: Option<String>,
    pub checkpointer: Option<CheckpointerType>,
    pub sqlite_db_name: Option<String>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            session_id: Some(id_generator::IdGenerator::new().generate_run_id()),
            checkpointer: Some(CheckpointerType::InMemory),
            sqlite_db_name: Self::resolve_sqlite_db_name(None),
        }
    }
}

impl RuntimeConfig {
    fn resolve_sqlite_db_name(provided: Option<String>) -> Option<String> {
        if let Some(name) = provided {
            return Some(name);
        }
        dotenvy::dotenv().ok();
        Some(std::env::var("SQLITE_DB_NAME").unwrap_or_else(|_| "weavegraph.db".to_string()))
    }

    pub fn new(
        session_id: Option<String>,
        checkpointer: Option<CheckpointerType>,
        sqlite_db_name: Option<String>,
    ) -> Self {
        Self {
            session_id,
            checkpointer,
            sqlite_db_name: Self::resolve_sqlite_db_name(sqlite_db_name),
        }
    }
}

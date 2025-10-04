use std::mem::transmute;
use std::os::raw::c_char;
use std::path::Path;

use once_cell::sync::OnceCell;
use rig::embeddings::{Embedding, EmbeddingModel};
use rig::OneOrMany;
use rig_sqlite::{
    Column, ColumnValue, SqliteVectorIndex, SqliteVectorStore, SqliteVectorStoreTable,
};
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use tokio_rusqlite::{ffi, Connection};

use crate::types::RagError;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChunkDocument {
    pub id: String,
    pub url: String,
    pub heading: String,
    #[serde(deserialize_with = "deserialize_chunk_index")]
    pub chunk_index: usize,
    pub content: String,
    #[serde(deserialize_with = "deserialize_metadata_field")]
    pub metadata: serde_json::Value,
}

impl SqliteVectorStoreTable for ChunkDocument {
    fn name() -> &'static str {
        "chunks"
    }

    fn schema() -> Vec<Column> {
        vec![
            Column::new("id", "TEXT PRIMARY KEY"),
            Column::new("url", "TEXT").indexed(),
            Column::new("heading", "TEXT"),
            Column::new("chunk_index", "TEXT"),
            Column::new("metadata", "TEXT"),
            Column::new("content", "TEXT"),
        ]
    }

    fn id(&self) -> String {
        self.id.clone()
    }

    fn column_values(&self) -> Vec<(&'static str, Box<dyn ColumnValue>)> {
        vec![
            ("id", Box::new(self.id.clone())),
            ("url", Box::new(self.url.clone())),
            ("heading", Box::new(self.heading.clone())),
            ("chunk_index", Box::new(self.chunk_index.to_string())),
            ("metadata", Box::new(self.metadata.to_string())),
            ("content", Box::new(self.content.clone())),
        ]
    }
}

fn deserialize_chunk_index<'de, D>(deserializer: D) -> Result<usize, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Repr {
        Num(u64),
        Text(String),
    }

    match Repr::deserialize(deserializer)? {
        Repr::Num(value) => usize::try_from(value)
            .map_err(|_| de::Error::custom(format!("chunk_index {value} does not fit in usize"))),
        Repr::Text(text) => text.parse::<usize>().map_err(|err| {
            de::Error::custom(format!("unable to parse chunk_index '{text}': {err}"))
        }),
    }
}

fn deserialize_metadata_field<'de, D>(deserializer: D) -> Result<serde_json::Value, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if let serde_json::Value::String(raw) = value {
        serde_json::from_str(&raw).map_or(Ok(serde_json::Value::String(raw)), Ok)
    } else {
        Ok(value)
    }
}

#[derive(Clone)]
pub struct SqliteChunkStore<E>
where
    E: EmbeddingModel + 'static,
{
    inner: SqliteVectorStore<E, ChunkDocument>,
}

impl<E> SqliteChunkStore<E>
where
    E: EmbeddingModel + Clone + Send + Sync + 'static,
{
    pub async fn open(path: impl AsRef<Path>, model: &E) -> Result<Self, RagError> {
        Self::register_sqlite_vec()?;
        let conn = Connection::open(path)
            .await
            .map_err(|err| RagError::Storage(err.to_string()))?;
        conn.call(|conn| {
            let result = conn.query_row("select vec_version()", [], |row| row.get::<_, String>(0));
            match result {
                Ok(_) => Ok(()),
                Err(err) => Err(tokio_rusqlite::Error::Rusqlite(err)),
            }
        })
        .await
        .map_err(|err| RagError::Storage(err.to_string()))?;
        let store = SqliteVectorStore::new(conn, model)
            .await
            .map_err(|err| RagError::Storage(err.to_string()))?;
        Ok(Self { inner: store })
    }

    pub async fn add_chunks(
        &self,
        documents: Vec<(ChunkDocument, Vec<f32>)>,
    ) -> Result<(), RagError> {
        if documents.is_empty() {
            return Ok(());
        }
        let mut rows = Vec::with_capacity(documents.len());
        for (doc, embedding) in documents {
            let converted: Vec<f64> = embedding.into_iter().map(|value| value as f64).collect();
            let embed = Embedding {
                document: doc.content.clone(),
                vec: converted,
            };
            rows.push((doc, OneOrMany::one(embed)));
        }
        self.inner
            .add_rows(rows)
            .await
            .map_err(|err| RagError::Storage(err.to_string()))?;
        Ok(())
    }

    fn register_sqlite_vec() -> Result<(), RagError> {
        static REGISTER: OnceCell<()> = OnceCell::new();
        REGISTER
            .get_or_try_init(|| {
                unsafe {
                    type SqliteExtensionInit = unsafe extern "C" fn(
                        *mut ffi::sqlite3,
                        *mut *mut c_char,
                        *const ffi::sqlite3_api_routines,
                    ) -> i32;

                    let init: unsafe extern "C" fn() = sqlite_vec::sqlite3_vec_init;
                    let init_fn: SqliteExtensionInit =
                        transmute::<unsafe extern "C" fn(), SqliteExtensionInit>(init);
                    let rc = ffi::sqlite3_auto_extension(Some(init_fn));
                    if rc != 0 {
                        return Err(RagError::Storage(format!(
                            "failed to register sqlite-vec extension (code {rc})"
                        )));
                    }
                }
                Ok(())
            })
            .map(|_| ())
    }

    pub fn index(&self, model: E) -> SqliteVectorIndex<E, ChunkDocument> {
        self.inner.clone().index(model)
    }

    pub fn store(&self) -> SqliteVectorStore<E, ChunkDocument> {
        self.inner.clone()
    }
}

use rig::OneOrMany;
use rig::embeddings::{Embedding, EmbeddingModel};
use rig_sqlite::{
    Column, ColumnValue, SqliteVectorIndex, SqliteVectorStore, SqliteVectorStoreTable,
};
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use std::mem::transmute;
use std::os::raw::c_char;
use std::path::Path;
use std::sync::Once;
use tokio_rusqlite::{Connection, OptionalExtension, ffi};

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
    /// Separate connection handle for direct queries not supported by rig-sqlite.
    /// This is a clone of the connection used by the inner store.
    conn: Connection,
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
        // Clone connection for direct access before moving into store
        let conn_for_queries = conn.clone();
        let store = SqliteVectorStore::new(conn, model)
            .await
            .map_err(|err| RagError::Storage(err.to_string()))?;
        Ok(Self {
            inner: store,
            conn: conn_for_queries,
        })
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
        use std::sync::Mutex;

        static INIT: Once = Once::new();
        static INIT_RESULT: Mutex<Option<Result<(), String>>> = Mutex::new(None);

        INIT.call_once(|| {
            let result = unsafe {
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
                    Err(format!(
                        "failed to register sqlite-vec extension (code {rc})"
                    ))
                } else {
                    Ok(())
                }
            };
            *INIT_RESULT.lock().expect("init result mutex poisoned") = Some(result);
        });

        INIT_RESULT
            .lock()
            .expect("init result mutex poisoned")
            .clone()
            .expect("init was called but result not set")
            .map_err(RagError::Storage)
    }

    pub fn index(&self, model: E) -> SqliteVectorIndex<E, ChunkDocument> {
        self.inner.clone().index(model)
    }

    pub fn store(&self) -> SqliteVectorStore<E, ChunkDocument> {
        self.inner.clone()
    }

    /// Get the underlying connection for direct queries.
    ///
    /// Use this for operations not covered by the `Backend` trait.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

// ============================================================================
// Backend Trait Implementation
// ============================================================================

use super::{Backend, ChunkRecord};
use async_trait::async_trait;

#[async_trait]
impl<E> Backend for SqliteChunkStore<E>
where
    E: EmbeddingModel + Clone + Send + Sync + 'static,
{
    async fn insert_chunks(&self, chunks: Vec<ChunkRecord>) -> Result<(), RagError> {
        if chunks.is_empty() {
            return Ok(());
        }

        let documents_with_embeddings: Vec<(ChunkDocument, Vec<f32>)> = chunks
            .into_iter()
            .filter_map(|record| {
                let embedding = record.embedding.clone()?;
                let doc = ChunkDocument::from(record);
                Some((doc, embedding))
            })
            .collect();

        self.add_chunks(documents_with_embeddings).await
    }

    async fn get_chunks_by_url(&self, url: &str) -> Result<Vec<ChunkRecord>, RagError> {
        let url = url.to_string();
        let conn = self.connection();

        conn.call(move |conn| {
            let mut stmt = conn
                .prepare("SELECT id, url, heading, chunk_index, content, metadata FROM chunks WHERE url = ?")
                .map_err(tokio_rusqlite::Error::Rusqlite)?;

            let rows = stmt
                .query_map([&url], |row| {
                    Ok(ChunkDocument {
                        id: row.get(0)?,
                        url: row.get(1)?,
                        heading: row.get(2)?,
                        chunk_index: row.get::<_, String>(3)?.parse().unwrap_or(0),
                        content: row.get(4)?,
                        metadata: row.get::<_, String>(5)
                            .map(|s| serde_json::from_str(&s).unwrap_or_default())
                            .unwrap_or_default(),
                    })
                })
                .map_err(tokio_rusqlite::Error::Rusqlite)?;

            let mut results = Vec::new();
            for row in rows {
                results.push(ChunkRecord::from(row.map_err(tokio_rusqlite::Error::Rusqlite)?));
            }
            Ok(results)
        })
        .await
        .map_err(|err| RagError::Storage(err.to_string()))
    }

    async fn get_chunk_by_id(&self, id: &str) -> Result<Option<ChunkRecord>, RagError> {
        let id = id.to_string();
        let conn = self.connection();

        conn.call(move |conn| {
            let mut stmt = conn
                .prepare("SELECT id, url, heading, chunk_index, content, metadata FROM chunks WHERE id = ?")
                .map_err(tokio_rusqlite::Error::Rusqlite)?;

            let result = stmt
                .query_row([&id], |row| {
                    Ok(ChunkDocument {
                        id: row.get(0)?,
                        url: row.get(1)?,
                        heading: row.get(2)?,
                        chunk_index: row.get::<_, String>(3)?.parse().unwrap_or(0),
                        content: row.get(4)?,
                        metadata: row.get::<_, String>(5)
                            .map(|s| serde_json::from_str(&s).unwrap_or_default())
                            .unwrap_or_default(),
                    })
                })
                .optional()
                .map_err(tokio_rusqlite::Error::Rusqlite)?;

            Ok(result.map(ChunkRecord::from))
        })
        .await
        .map_err(|err| RagError::Storage(err.to_string()))
    }

    async fn delete_chunks_by_url(&self, url: &str) -> Result<usize, RagError> {
        let url = url.to_string();
        let conn = self.connection();

        conn.call(move |conn| {
            let deleted = conn
                .execute("DELETE FROM chunks WHERE url = ?", [&url])
                .map_err(tokio_rusqlite::Error::Rusqlite)?;
            Ok(deleted)
        })
        .await
        .map_err(|err| RagError::Storage(err.to_string()))
    }

    async fn search_similar(
        &self,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<(ChunkRecord, f32)>, RagError> {
        // For similarity search, we need to use the rig vector index
        // The index requires an embedding model to convert text to embeddings,
        // but since we already have the query embedding, we use the raw SQL approach
        let embedding_json = serde_json::to_string(query_embedding)
            .map_err(|err| RagError::Storage(err.to_string()))?;
        let conn = self.connection();

        conn.call(move |conn| {
            // Use sqlite-vec for cosine distance search
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT c.id, c.url, c.heading, c.chunk_index, c.content, c.metadata, \
                     vec_distance_cosine(e.embedding, vec_f32(?)) as distance \
                     FROM chunks c \
                     JOIN chunks_embeddings e ON c.id = e.id \
                     ORDER BY distance ASC \
                     LIMIT {}",
                    top_k
                ))
                .map_err(tokio_rusqlite::Error::Rusqlite)?;

            let rows = stmt
                .query_map([&embedding_json], |row| {
                    let doc = ChunkDocument {
                        id: row.get(0)?,
                        url: row.get(1)?,
                        heading: row.get(2)?,
                        chunk_index: row.get::<_, String>(3)?.parse().unwrap_or(0),
                        content: row.get(4)?,
                        metadata: row
                            .get::<_, String>(5)
                            .map(|s| serde_json::from_str(&s).unwrap_or_default())
                            .unwrap_or_default(),
                    };
                    let distance: f32 = row.get(6)?;
                    // Convert distance to similarity (1 - distance for cosine)
                    let similarity = 1.0 - distance;
                    Ok((ChunkRecord::from(doc), similarity))
                })
                .map_err(tokio_rusqlite::Error::Rusqlite)?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.map_err(tokio_rusqlite::Error::Rusqlite)?);
            }
            Ok(results)
        })
        .await
        .map_err(|err| RagError::Storage(err.to_string()))
    }

    async fn count(&self) -> Result<usize, RagError> {
        let conn = self.connection();

        conn.call(|conn| {
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
                .map_err(tokio_rusqlite::Error::Rusqlite)?;
            Ok(count as usize)
        })
        .await
        .map_err(|err| RagError::Storage(err.to_string()))
    }
}

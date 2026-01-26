//! Demonstrates querying chunks and embeddings from a SQLite vector database.
//!
//! This example shows how to:
//! - Register the sqlite-vec extension using std::sync::OnceLock for one-time init
//! - Query chunk documents from a SQLite database
//! - Query vector embeddings using vec0 virtual tables
//! - Use sqlite-vec functions like vec_to_json()
//!
//! The example automatically creates test data if the database doesn't exist or is empty,
//! making it runnable without any setup:
//!
//! ```bash
//! cargo run --example query_chunks
//! ```

use std::sync::OnceLock;
use std::mem::transmute;
use std::os::raw::c_char;
use tokio_rusqlite::{Connection, Result, ffi};

#[tokio::main]
async fn main() -> Result<()> {
    // Register sqlite-vec extension
    register_sqlite_vec();

    let conn = Connection::open("rust_book_chunks.sqlite").await?;

    // Verify sqlite-vec is working
    let version = conn
        .call(|conn| {
            match conn.query_row("select vec_version()", [], |row| row.get::<_, String>(0)) {
                Ok(v) => Ok(v),
                Err(e) => Err(tokio_rusqlite::Error::Rusqlite(e)),
            }
        })
        .await;

    match version {
        Ok(v) => println!("sqlite-vec version: {}", v),
        Err(e) => {
            println!("Failed to get sqlite-vec version: {}", e);
            return Ok(());
        }
    }

    // Check if database has data, if not create test data
    let needs_setup = conn
        .call(|conn| {
            match conn.query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='chunks'",
                [],
                |_| Ok(()),
            ) {
                Ok(_) => {
                    // Table exists, check if it has data
                    match conn.query_row("SELECT COUNT(*) FROM chunks", [], |row| {
                        row.get::<_, i64>(0)
                    }) {
                        Ok(count) => Ok(count == 0),
                        Err(_) => Ok(true),
                    }
                }
                Err(_) => Ok(true),
            }
        })
        .await?;

    if needs_setup {
        println!("\nNo data found. Creating test database...\n");
        setup_test_database(&conn).await?;
    }

    // Let's try a simpler query first - just get chunks without embeddings
    let results = conn
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT c.id,
                    substr(replace(c.content, char(10), ' '), 1, 80) AS preview
             FROM chunks AS c
             LIMIT 5",
            )?;

            let chunk_iter = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?, // id
                    row.get::<_, String>(1)?, // preview
                ))
            })?;

            let mut chunks = Vec::new();
            for chunk in chunk_iter {
                chunks.push(chunk?);
            }
            Ok(chunks)
        })
        .await?;

    println!("ID                                    | Preview");
    println!(
        "--------------------------------------|--------------------------------------------------"
    );

    for (id, preview) in results {
        println!("{:<37} | {}", id, preview);
    }

    // Try to get count from embeddings table (this will fail without extension)
    let embedding_result = conn
        .call(move |conn| {
            match conn.query_row("SELECT COUNT(*) FROM chunks_embeddings", [], |row| {
                row.get::<_, i64>(0)
            }) {
                Ok(count) => Ok(count),
                Err(e) => Err(tokio_rusqlite::Error::Rusqlite(e)),
            }
        })
        .await;

    match embedding_result {
        Ok(count) => {
            println!("\nTotal embeddings: {}", count);
        }
        Err(e) => {
            println!("\nCould not access embeddings table: {}", e);
        }
    }

    // Now let's try the original query with vec_to_json
    let embedding_query_result = conn
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT c.id,
                    substr(replace(c.content, char(10), ' '), 1, 80) AS preview,
                    substr(vec_to_json(e.embedding), 1, 80) || ' …' AS embedding_preview
             FROM   chunks            AS c
             JOIN   chunks_embeddings AS e
                    ON e.rowid = c.rowid
             LIMIT 5",
            )?;

            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?, // id
                    row.get::<_, String>(1)?, // preview
                    row.get::<_, String>(2)?, // embedding_preview
                ))
            })?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row?);
            }
            Ok(results)
        })
        .await;

    match embedding_query_result {
        Ok(results) => {
            println!("\n=== Chunks with Embeddings ===");
            println!("{:<37} | {:<48} | Embedding Preview", "ID", "Preview");
            println!("{:-<37}-|-{:-<48}-|{:-<50}", "", "", "");
            for (id, preview, embedding_preview) in results {
                println!("{:<37} | {:<48} | {}", id, preview, embedding_preview);
            }
        }
        Err(e) => {
            println!("\nFailed to query embeddings with vec_to_json: {}", e);
        }
    }

    Ok(())
}

fn register_sqlite_vec() {
    static REGISTERED: OnceLock<()> = OnceLock::new();

    REGISTERED.get_or_init(|| {
        unsafe {
            type SqliteExtensionInit = unsafe extern "C" fn(
                *mut ffi::sqlite3,
                *mut *mut c_char,
                *const ffi::sqlite3_api_routines,
            ) -> i32;

            let init: unsafe extern "C" fn() = sqlite_vec::sqlite3_vec_init;
            let init_fn: SqliteExtensionInit = transmute::<_, SqliteExtensionInit>(init);
            let rc = ffi::sqlite3_auto_extension(Some(init_fn));
            // Panic on error instead of returning Result
            // registration failure is a fatal initialization error that should fail fast
            if rc != ffi::SQLITE_OK {
                panic!("failed to register sqlite-vec extension (code {rc})");
            }
        }
    });
}

async fn setup_test_database(conn: &Connection) -> Result<()> {
    // Create chunks table
    conn.call(|conn| {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS chunks (
                id TEXT PRIMARY KEY,
                url TEXT,
                heading TEXT,
                chunk_index TEXT,
                metadata TEXT,
                content TEXT
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_chunks_url ON chunks(url)",
            [],
        )?;
        Ok(())
    })
    .await?;

    // Insert test chunks
    conn.call(|conn| {
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO chunks (id, url, heading, chunk_index, metadata, content) VALUES 
            ('test-1', 'https://example.com/doc1', 'Introduction', '0', '{}', 'This is the first chunk of content about Rust programming.')",
            [],
        )?;
        tx.execute(
            "INSERT INTO chunks (id, url, heading, chunk_index, metadata, content) VALUES 
            ('test-2', 'https://example.com/doc1', 'Introduction', '1', '{}', 'This is the second chunk explaining basic concepts.')",
            [],
        )?;
        tx.execute(
            "INSERT INTO chunks (id, url, heading, chunk_index, metadata, content) VALUES 
            ('test-3', 'https://example.com/doc2', 'Advanced Topics', '0', '{}', 'Advanced Rust features including lifetimes and traits.')",
            [],
        )?;
        tx.execute(
            "INSERT INTO chunks (id, url, heading, chunk_index, metadata, content) VALUES 
            ('test-4', 'https://example.com/doc2', 'Advanced Topics', '1', '{}', 'More advanced topics covering async/await patterns.')",
            [],
        )?;
        tx.execute(
            "INSERT INTO chunks (id, url, heading, chunk_index, metadata, content) VALUES 
            ('test-5', 'https://example.com/doc3', 'Best Practices', '0', '{}', 'Best practices for writing idiomatic Rust code.')",
            [],
        )?;
        tx.commit()?;
        Ok(())
    })
    .await?;

    // Create embeddings table using vec0
    conn.call(|conn| {
        // Drop if exists (in case of dimension mismatch)
        let _ = conn.execute("DROP TABLE IF EXISTS chunks_embeddings", []);

        conn.execute(
            "CREATE VIRTUAL TABLE chunks_embeddings USING vec0(embedding float[3])",
            [],
        )?;
        Ok(())
    })
    .await?;

    // Insert test embeddings
    conn.call(|conn| {
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO chunks_embeddings (rowid, embedding) VALUES (1, '[0.1, 0.2, 0.3]')",
            [],
        )?;
        tx.execute(
            "INSERT INTO chunks_embeddings (rowid, embedding) VALUES (2, '[0.4, 0.5, 0.6]')",
            [],
        )?;
        tx.execute(
            "INSERT INTO chunks_embeddings (rowid, embedding) VALUES (3, '[0.7, 0.8, 0.9]')",
            [],
        )?;
        tx.execute(
            "INSERT INTO chunks_embeddings (rowid, embedding) VALUES (4, '[0.2, 0.3, 0.4]')",
            [],
        )?;
        tx.execute(
            "INSERT INTO chunks_embeddings (rowid, embedding) VALUES (5, '[0.5, 0.6, 0.7]')",
            [],
        )?;
        tx.commit()?;
        Ok(())
    })
    .await?;

    println!("✓ Created chunks table with 5 test documents");
    println!("✓ Created embeddings table with 5 test vectors");

    Ok(())
}

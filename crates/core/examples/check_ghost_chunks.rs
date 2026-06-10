use doxus_core::db;
use std::path::PathBuf;

fn main() {
    let home = std::env::var("HOME").unwrap();
    let db_path = PathBuf::from(home).join(".doxus/db/doxus.db");
    let conn = db::open(&db_path).expect("failed to open db");

    let orphan_embeddings: i64 = conn
        .query_row(
            "SELECT count(*) FROM chunk_embeddings WHERE chunk_id NOT IN (SELECT id FROM chunks)",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    println!("Orphan embeddings: {}", orphan_embeddings);

    let orphan_fts: i64 = conn
        .query_row(
            "SELECT count(*) FROM chunks_fts WHERE rowid NOT IN (SELECT id FROM chunks)",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    println!("Orphan FTS entries: {}", orphan_fts);

    let total_chunks: i64 = conn
        .query_row("SELECT count(*) FROM chunks", [], |r| r.get(0))
        .unwrap();
    let total_embeddings: i64 = conn
        .query_row("SELECT count(*) FROM chunk_embeddings", [], |r| r.get(0))
        .unwrap();
    let total_fts: i64 = conn
        .query_row("SELECT count(*) FROM chunks_fts", [], |r| r.get(0))
        .unwrap();

    println!("Total chunks: {}", total_chunks);
    println!("Total embeddings: {}", total_embeddings);
    println!("Total FTS: {}", total_fts);
}

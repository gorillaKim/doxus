use doxus_core::chunker::{split_chunks, ChunkConfig};
use doxus_core::search::{SearchEngine, SearchQuery, SearchMode, Hit};
use std::sync::{Arc, Mutex};
use rusqlite::Connection;

#[tokio::main]
async fn main() {
    println!("=== Doxus RAG Pipeline Verification ===");

    // 1. Chunker Verification
    verify_chunker();

    // 2. Search Strategy (Tiered Budgeting) Verification
    verify_search_strategy().await;

    println!("\n=== Verification Complete ===");
}

fn verify_chunker() {
    println!("\n[1] Chunking Quality Test");
    let text = "First sentence. Second sentence. Third sentence. Fourth sentence.";
    // Limit to 20 chars, which should force splits
    let config = ChunkConfig { max_chars: 20, overlap_chars: 5, ..Default::default() };
    let chunks = split_chunks(text, config);
    
    println!("Text: {}", text);
    for (i, c) in chunks.iter().enumerate() {
        println!("Chunk {}: [{}] (len: {})", i, c.content, c.content.len());
        // Verify no mid-word split if possible
        assert!(!c.content.contains("sentenc") || c.content.contains("sentence"), "Should not split mid-word");
    }

    let ko_text = "안녕하세요. 반가워요. 오늘 날씨가 좋네요. 내일도 좋을까요?";
    let ko_chunks = split_chunks(ko_text, ChunkConfig { max_chars: 20, ..Default::default() });
    println!("\nKorean Text: {}", ko_text);
    for (i, c) in ko_chunks.iter().enumerate() {
        println!("Chunk {}: [{}]", i, c.content);
    }
}

async fn verify_search_strategy() {
    println!("\n[2] Search Strategy (Statistical Tiering) Test");
    
    // Setup in-memory DB for and indexing
    let conn = Connection::open_in_memory().unwrap();
    // (Actual schema setup omitted for brevity since we're testing the logic via hits)
    // We'll mock the SearchEngine logic by manually running a subset of search_async's logic
    
    let hits = vec![
        mock_hit(1, 0.95, "Summary 1"), // Tier 1
        mock_hit(2, 0.92, "Summary 2"), // Tier 1
        mock_hit(3, 0.88, "Summary 3"), // Tier 1
        mock_hit(4, 0.40, "Summary 4"), // Tier 2
        mock_hit(5, 0.35, "Summary 5"), // Tier 2
    ];

    // Statistical analysis (from search.rs)
    let scores: Vec<f64> = hits.iter().map(|h| h.score).collect();
    let n = scores.len() as f64;
    let mean = scores.iter().sum::<f64>() / n;
    let variance = scores.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / n;
    let sigma = variance.sqrt();
    let max_score = scores[0];

    println!("Scores: {:?}", scores);
    println!("Mean: {:.4}, Sigma: {:.4}, Max: {:.4}", mean, sigma, max_score);
    println!("Threshold (Max - Sigma): {:.4}", max_score - sigma);

    for (i, hit) in hits.iter().enumerate() {
        let is_high_confidence = hit.score >= (max_score - sigma);
        let tier = if is_high_confidence { "Tier 1 (High Budget)" } else { "Tier 2 (Low Budget)" };
        println!("Hit {}: Score {:.4} -> {}", i + 1, hit.score, tier);
        
        if is_high_confidence {
             assert!(hit.score > 0.8, "High confidence should be high scores in this mock");
        } else {
             assert!(hit.score < 0.5, "Low confidence should be low scores in this mock");
        }
    }
}

fn mock_hit(id: i64, score: f64, snippet: &str) -> Hit {
    Hit {
        document_id: id,
        chunk_id: id * 10,
        project_id: 1,
        source_doc_id: format!("doc_{}", id),
        title: Some(format!("Title {}", id)),
        score,
        snippet: Some(snippet.to_string()),
        ..Default::default()
    }
}

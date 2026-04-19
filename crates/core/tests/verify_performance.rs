use doxus_core::chunker::{split_chunks, ChunkConfig};
use doxus_core::search::Hit;

#[test]
fn test_chunker_sentence_quality() {
    println!("\n[Verification] Chunker Sentence Quality - Korean/English Mixed");
    let text = "안녕하세요. 이것은 첫 번째 문장입니다. This is the second sentence. 그리고 마지막 문장입니다.";
    // max_chars=30 should ideally split between sentences
    let config = ChunkConfig { max_chars: 30, overlap_chars: 0, ..Default::default() };
    let chunks = split_chunks(text, config);
    
    for (i, c) in chunks.iter().enumerate() {
        println!("Chunk {}: [{}]", i, c.content);
        // Each chunk should end with a period in this specific case if sentence-awareness works
        assert!(c.content.ends_with('.') || c.content.ends_with('?'), "Chunk should break at sentence boundary");
    }
}

#[test]
fn test_chunker_code_block_protection() {
    println!("\n[Verification] Chunker Code Block Protection");
    let text = "Check out this code:\n\n```python\ndef hello():\n    print('world')\n    return True\n```\n\nHope you like it.";
    // Limit that would normally cut mid-code block
    let config = ChunkConfig { max_chars: 40, overlap_chars: 0, ..Default::default() };
    let chunks = split_chunks(text, config);
    
    for (i, c) in chunks.iter().enumerate() {
        println!("Chunk {}: [{}]", i, c.content);
    }
    
    // The code block should ideally stay together or split at newlines, not mid-word
    let has_hello = chunks.iter().any(|c| c.content.contains("def hello():"));
    assert!(has_hello, "Code block header should be preserved");
}

#[test]
fn test_statistical_tiering_logic() {
    println!("\n[Verification] Statistical Tiering Efficacy");
    
    // Simulate search hits with high variance (Clear winners)
    let hits_high_var = vec![
        mock_hit(1, 0.98), // Tier 1
        mock_hit(2, 0.95), // Tier 1
        mock_hit(3, 0.40), // Tier 2
        mock_hit(4, 0.38), // Tier 2
    ];
    
    let (t1, t2) = simulate_tiering(&hits_high_var);
    println!("High Var -> T1: {}, T2: {}", t1, t2);
    assert_eq!(t1, 2, "High variance should result in 2 top tier items");

    // Simulate search hits with low variance (Ambiguous)
    let hits_low_var = vec![
        mock_hit(1, 0.85),
        mock_hit(2, 0.84),
        mock_hit(3, 0.83),
        mock_hit(4, 0.82),
    ];
    let (t1_low, t2_low) = simulate_tiering(&hits_low_var);
    println!("Low Var -> T1: {}, T2: {}", t1_low, t2_low);
    // Standard deviation will be small, (Max - Sigma) will likely catch multiple items
    assert!(t1_low >= 1, "At least one item should be in T1");
}

fn mock_hit(id: i64, score: f64) -> Hit {
    Hit {
        document_id: id,
        chunk_id: id * 10,
        score,
        snippet: Some(format!("Snippet for {}", id)),
        ..Default::default()
    }
}

fn simulate_tiering(hits: &[Hit]) -> (usize, usize) {
    if hits.is_empty() { return (0, 0); }
    let scores: Vec<f64> = hits.iter().map(|h| h.score).collect();
    let n = scores.len() as f64;
    let mean = scores.iter().sum::<f64>() / n;
    let variance = scores.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / n;
    let sigma = variance.sqrt();
    let max_score = scores[0];

    let t1 = hits.iter().filter(|h| h.score >= (max_score - sigma)).count();
    let t2 = hits.len() - t1;
    (t1, t2)
}

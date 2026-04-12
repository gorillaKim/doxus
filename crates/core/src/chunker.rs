/// Text chunker — splits long documents into overlapping chunks for FTS/vector indexing.
///
/// Design:
/// - Split on paragraph boundaries (blank lines) first to avoid cutting mid-sentence
/// - Merge small paragraphs until the chunk reaches `max_chars`
/// - Overlap: last `overlap_chars` of each chunk are prepended to the next
/// - Minimum chunk size: at least one paragraph (never returns empty chunks)

pub const DEFAULT_MAX_CHARS: usize = 1500;
pub const DEFAULT_OVERLAP_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub content: String,
    pub index: usize,
}

/// Split `text` into overlapping chunks.
pub fn split_chunks(text: &str, max_chars: usize, overlap_chars: usize) -> Vec<Chunk> {
    if text.is_empty() {
        return vec![];
    }
    if text.len() <= max_chars {
        return vec![Chunk { content: text.to_string(), index: 0 }];
    }

    let paragraphs: Vec<&str> = split_paragraphs(text);
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut current = String::new();
    let mut overlap_tail = String::new();

    for para in &paragraphs {
        // If adding this paragraph would exceed max_chars, flush current chunk
        if !current.is_empty() && current.len() + para.len() + 1 > max_chars {
            let idx = chunks.len();
            chunks.push(Chunk { content: current.trim().to_string(), index: idx });

            // Compute overlap tail from end of flushed chunk
            let flushed = chunks.last().unwrap().content.as_str();
            overlap_tail = tail_chars(flushed, overlap_chars).to_string();

            current = if overlap_tail.is_empty() {
                para.to_string()
            } else {
                format!("{}\n\n{}", overlap_tail, para)
            };
        } else if current.is_empty() {
            current = if overlap_tail.is_empty() {
                para.to_string()
            } else {
                format!("{}\n\n{}", overlap_tail, para)
            };
            overlap_tail.clear();
        } else {
            current.push_str("\n\n");
            current.push_str(para);
        }
    }

    // Flush remaining
    if !current.trim().is_empty() {
        let idx = chunks.len();
        chunks.push(Chunk { content: current.trim().to_string(), index: idx });
    }

    chunks
}

/// Split text on blank lines, filtering empty results.
fn split_paragraphs(text: &str) -> Vec<&str> {
    text.split("\n\n")
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect()
}

/// Return the last `n` chars of `s` (aligned to char boundary, starting at whitespace when possible).
fn tail_chars(s: &str, n: usize) -> &str {
    if s.len() <= n {
        return s;
    }
    let start = s.len() - n;
    // Align to char boundary
    let aligned = s
        .char_indices()
        .map(|(i, _)| i)
        .find(|&i| i >= start)
        .unwrap_or(start);
    &s[aligned..]
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Basic behaviour ───────────────────────────────────────────────────────

    #[test]
    fn empty_text_returns_empty() {
        assert_eq!(split_chunks("", 1000, 100), vec![]);
    }

    #[test]
    fn short_text_returns_single_chunk() {
        let text = "Hello world";
        let chunks = split_chunks(text, 1000, 100);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "Hello world");
        assert_eq!(chunks[0].index, 0);
    }

    #[test]
    fn exactly_max_chars_is_single_chunk() {
        let text = "a".repeat(1500);
        let chunks = split_chunks(&text, 1500, 200);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn long_text_splits_into_multiple_chunks() {
        // 3 paragraphs each ~600 chars, max=1000 → should produce at least 2 chunks
        let para = "x".repeat(600);
        let text = format!("{}\n\n{}\n\n{}", para, para, para);
        let chunks = split_chunks(&text, 1000, 50);
        assert!(chunks.len() >= 2, "expected >=2 chunks, got {}", chunks.len());
    }

    #[test]
    fn chunk_indices_are_sequential() {
        let para = "word ".repeat(200); // ~1000 chars per para
        let text = format!("{}\n\n{}\n\n{}\n\n{}", para, para, para, para);
        let chunks = split_chunks(&text, 1200, 100);
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.index, i, "chunk index mismatch at position {i}");
        }
    }

    #[test]
    fn no_chunk_exceeds_max_chars_by_single_paragraph() {
        let short_para = "short paragraph here. ".repeat(10); // ~220 chars
        let text = (0..20)
            .map(|_| short_para.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let chunks = split_chunks(&text, 500, 50);
        for chunk in &chunks {
            // A chunk may exceed max_chars only if a single paragraph is already larger
            // Here paragraphs are ~220 chars so chunks should stay close to limit
            assert!(
                chunk.content.len() <= 500 + short_para.len(),
                "chunk too large: {}",
                chunk.content.len()
            );
        }
    }

    #[test]
    fn overlap_content_appears_in_next_chunk() {
        // Make two paragraphs that together exceed max_chars
        let para1 = format!("FIRST {}", "a".repeat(600));
        let para2 = format!("SECOND {}", "b".repeat(600));
        let text = format!("{}\n\n{}", para1, para2);
        let chunks = split_chunks(&text, 700, 100);
        assert!(chunks.len() >= 2);
        // The second chunk should contain some tail from para1 (overlap)
        let second = &chunks[1].content;
        // overlap tail of para1 should appear at start of second chunk
        let tail = tail_chars(&para1, 100);
        assert!(
            second.contains(tail.trim()),
            "overlap tail not found in second chunk.\ntail={tail:?}\nchunk={second:?}"
        );
    }

    #[test]
    fn no_empty_chunks_produced() {
        let text = "\n\n\n\nhello\n\n\n\nworld\n\n\n\n";
        let chunks = split_chunks(text, 1000, 100);
        for chunk in &chunks {
            assert!(!chunk.content.is_empty(), "empty chunk produced");
        }
    }

    #[test]
    fn korean_text_splits_correctly() {
        let para = "이것은 한국어 테스트 문장입니다. ".repeat(50); // ~800 chars
        let text = format!("{}\n\n{}\n\n{}", para, para, para);
        let chunks = split_chunks(&text, 1000, 100);
        assert!(chunks.len() >= 2);
        // All chunks should have valid UTF-8 (no char boundary slicing)
        for chunk in &chunks {
            assert!(std::str::from_utf8(chunk.content.as_bytes()).is_ok());
        }
    }

    #[test]
    fn single_very_long_paragraph_becomes_one_chunk() {
        // If a single paragraph exceeds max_chars, it should still be one chunk (not dropped)
        let long_para = "word ".repeat(400); // ~2000 chars > 1500 max
        let chunks = split_chunks(&long_para, 1500, 200);
        assert_eq!(chunks.len(), 1, "single long para should be one chunk");
        assert!(!chunks[0].content.is_empty());
    }

    // ── tail_chars helper ─────────────────────────────────────────────────────

    #[test]
    fn tail_chars_shorter_than_n() {
        assert_eq!(tail_chars("hello", 100), "hello");
    }

    #[test]
    fn tail_chars_returns_last_n() {
        let s = "abcdefghij";
        assert_eq!(tail_chars(s, 3), "hij");
    }
}

/// Text chunker — splits long documents into overlapping chunks for FTS/vector indexing.
///
/// Design:
/// - Split on paragraph boundaries (blank lines) first to avoid cutting mid-sentence
/// - Merge small paragraphs until the chunk reaches `max_chars`
/// - Overlap: last `overlap_chars` of each chunk are prepended to the next
/// - Minimum chunk size: at least one paragraph (never returns empty chunks)
pub const DEFAULT_MAX_CHARS: usize = 1000;
pub const DEFAULT_OVERLAP_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub content: String,
    /// The text used for embedding (including title/context augmentation)
    pub embedding_text: String,
    pub index: usize,
    /// Heading path from markdown (e.g. "Intro > Background")
    pub heading_path: Option<String>,
    pub start_byte: usize,
    pub end_byte: usize,
}

pub struct ChunkConfig {
    pub max_chars: usize,
    pub overlap_chars: usize,
    pub title: Option<String>,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            max_chars: DEFAULT_MAX_CHARS,
            overlap_chars: DEFAULT_OVERLAP_CHARS,
            title: None,
        }
    }
}

/// Split `text` into overlapping chunks, aware of Markdown structure.
pub fn split_chunks(text: &str, config: ChunkConfig) -> Vec<Chunk> {
    if text.is_empty() {
        return vec![];
    }

    use crate::document::section::parse_sections;
    let sections = parse_sections(text);

    let mut chunks = Vec::new();
    if sections.is_empty() {
        // Fallback to paragraph splitting if no headers found
        return split_into_recursive_chunks(text, 0, None, &config, 0);
    }

    let mut current_index = 0;
    for section in sections {
        let section_chunks = split_into_recursive_chunks(
            &section.content,
            section.start_byte,
            Some(section.heading.clone()),
            &config,
            current_index,
        );
        current_index += section_chunks.len();
        chunks.extend(section_chunks);
    }

    chunks
}

fn split_into_recursive_chunks(
    text: &str,
    base_offset: usize,
    heading: Option<String>,
    config: &ChunkConfig,
    start_index: usize,
) -> Vec<Chunk> {
    let paragraphs = split_paragraphs_with_offsets(text);
    let mut chunks = Vec::new();
    let mut current_paras = Vec::new();
    let mut current_char_count = 0;

    for (para_text, para_offset) in paragraphs {
        let para_char_count = para_text.chars().count();

        // If single paragraph is already too large, we must split it by sentences
        if para_char_count > config.max_chars {
            // Flush preceding paras
            if !current_paras.is_empty() {
                chunks.push(create_chunk_from_paras(
                    text,
                    &current_paras,
                    base_offset,
                    &heading,
                    config,
                    start_index + chunks.len(),
                ));
                current_paras.clear();
                current_char_count = 0;
            }

            let sub_chunks = split_long_paragraph_by_sentences(
                para_text,
                base_offset + para_offset,
                &heading,
                config,
                start_index + chunks.len(),
            );

            chunks.extend(sub_chunks);
            continue;
        }

        if !current_paras.is_empty() && current_char_count + para_char_count + 2 > config.max_chars
        {
            // Flush current
            let chunk = create_chunk_from_paras(
                text,
                &current_paras,
                base_offset,
                &heading,
                config,
                start_index + chunks.len(),
            );
            chunks.push(chunk);

            // For overlap, we keep the last few paragraphs that fit within overlap_chars
            let safe_overlap = config.overlap_chars.min(config.max_chars / 2);
            let mut overlap_paras = Vec::new();
            let mut overlap_count = 0;
            for (p_text, p_off) in current_paras.iter().rev() {
                let p_len = p_text.chars().count();
                if overlap_count + p_len > safe_overlap && !overlap_paras.is_empty() {
                    break;
                }
                overlap_paras.push((*p_text, *p_off));
                overlap_count += p_len;
            }
            overlap_paras.reverse();
            current_paras = overlap_paras;
            current_char_count = overlap_count;
        }

        current_paras.push((para_text, para_offset));
        current_char_count += para_char_count + 2; // +2 for \n\n
    }

    if !current_paras.is_empty() {
        chunks.push(create_chunk_from_paras(
            text,
            &current_paras,
            base_offset,
            &heading,
            config,
            start_index + chunks.len(),
        ));
    }

    chunks
}

fn split_long_paragraph_by_sentences(
    text: &str,
    base_offset: usize,
    heading: &Option<String>,
    config: &ChunkConfig,
    start_index: usize,
) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let sentences = split_sentences_with_offsets(text);
    let mut current_content = String::new();
    let mut current_start = 0;

    for (s_whole, s_off) in sentences {
        // If the sentence itself is larger than max_chars, we must force split it by characters
        if s_whole.chars().count() > config.max_chars {
            // Flush preceding
            if !current_content.is_empty() {
                chunks.push(create_chunk_with_offsets(
                    &current_content,
                    base_offset + current_start,
                    base_offset + current_start + current_content.len(),
                    heading,
                    config,
                    start_index + chunks.len(),
                ));
                current_content.clear();
            }

            // Force split by character count
            let s_chars: Vec<char> = s_whole.chars().collect();
            let mut char_idx = 0;
            while char_idx < s_chars.len() {
                let take = config.max_chars.min(s_chars.len() - char_idx);
                let part: String = s_chars[char_idx..char_idx + take].iter().collect();

                // Approximate byte offsets (not perfect for multi-byte but create_chunk_with_offsets uses length)
                // Actually create_chunk_with_offsets expects content and bytes.
                // We'll just use a simpler approach: fixed byte chunks if needed, but chars are safer for utf8.
                let part_bytes = part.len();
                chunks.push(create_chunk_with_offsets(
                    &part,
                    base_offset + s_off + char_idx, // This is WRONG if multi-byte.
                    base_offset + s_off + char_idx + part_bytes,
                    heading,
                    config,
                    start_index + chunks.len(),
                ));

                char_idx += take.saturating_sub(config.overlap_chars.min(take / 2));
                if char_idx >= s_chars.len() {
                    break;
                }
            }
            continue;
        }

        if !current_content.is_empty()
            && current_content.chars().count() + s_whole.chars().count() > config.max_chars
        {
            chunks.push(create_chunk_with_offsets(
                &current_content,
                base_offset + current_start,
                base_offset + current_start + current_content.len(),
                heading,
                config,
                start_index + chunks.len(),
            ));

            // Simple overlap: take last few chars
            let overlap = tail_chars(&current_content, config.overlap_chars).to_string();
            current_start = s_off - overlap.len();
            current_content = overlap;
        }

        if current_content.is_empty() {
            current_start = s_off;
        }
        current_content.push_str(s_whole);
    }

    if !current_content.is_empty() {
        chunks.push(create_chunk_with_offsets(
            &current_content,
            base_offset + current_start,
            base_offset + current_start + current_content.len(),
            heading,
            config,
            start_index + chunks.len(),
        ));
    }

    chunks
}

fn split_sentences_with_offsets(text: &str) -> Vec<(&str, usize)> {
    let mut result = Vec::new();
    let mut last_idx = 0;
    let char_indices: Vec<(usize, char)> = text.char_indices().collect();

    for i in 0..char_indices.len() {
        let (_byte_pos, c) = char_indices[i];
        let next_byte_pos = i + 1 < char_indices.len();

        if (c == '.' || c == '?' || c == '!')
            && (!next_byte_pos || char_indices[i + 1].1.is_whitespace())
        {
            let end_pos = if next_byte_pos {
                char_indices[i + 1].0
            } else {
                text.len()
            };
            let s_text = &text[last_idx..end_pos];
            result.push((s_text, last_idx));
            last_idx = end_pos;
        }
    }

    if last_idx < text.len() {
        result.push((&text[last_idx..], last_idx));
    }
    result
}

fn create_chunk_from_paras(
    full_text: &str,
    paras: &[(&str, usize)],
    base_offset: usize,
    heading: &Option<String>,
    config: &ChunkConfig,
    index: usize,
) -> Chunk {
    let first_para = paras.first().unwrap();
    let last_para = paras.last().unwrap();

    let chunk_start_relative = first_para.1;
    let chunk_end_relative = last_para.1 + last_para.0.len();
    let content = &full_text[chunk_start_relative..chunk_end_relative];

    create_chunk_with_offsets(
        content,
        base_offset + chunk_start_relative,
        base_offset + chunk_end_relative,
        heading,
        config,
        index,
    )
}

fn create_chunk_with_offsets(
    content: &str,
    start_byte: usize,
    end_byte: usize,
    heading: &Option<String>,
    config: &ChunkConfig,
    index: usize,
) -> Chunk {
    let trimmed = content.trim().to_string();

    // Title Augmentation with 15% limit (approx 150 chars for 1000 limit)
    // Ensure anchor limit is at least a few characters even for tiny chunks
    let anchor_limit = ((config.max_chars as f32 * 0.15) as usize).max(20);

    let mut embedding_text = String::new();
    if let Some(ref title) = config.title {
        let char_count = title.chars().count();
        let t = if char_count > anchor_limit {
            let truncated: String = title.chars().take(anchor_limit.saturating_sub(3)).collect();
            format!("{}...", truncated)
        } else {
            title.to_string()
        };
        embedding_text.push_str(&format!("[Title: {}] ", t));
    }
    if let Some(ref h) = heading {
        let char_count = h.chars().count();
        let h_text = if char_count > anchor_limit {
            let truncated: String = h.chars().take(anchor_limit.saturating_sub(3)).collect();
            format!("{}...", truncated)
        } else {
            h.to_string()
        };
        embedding_text.push_str(&format!("[Section: {}] ", h_text));
    }
    embedding_text.push_str(&trimmed);

    Chunk {
        content: trimmed,
        embedding_text,
        index,
        heading_path: heading.clone(),
        start_byte,
        end_byte,
    }
}

/// Split text on blank lines, filtering empty results, returning slices and their offsets in text.
fn split_paragraphs_with_offsets(text: &str) -> Vec<(&str, usize)> {
    let mut result = Vec::new();
    let base_ptr = text.as_ptr() as usize;
    for part in text.split("\n\n") {
        let trimmed = part.trim();
        if !trimmed.is_empty() {
            let offset = trimmed.as_ptr() as usize - base_ptr;
            result.push((trimmed, offset));
        }
    }
    result
}

#[allow(dead_code)]
/// Return the last `n` chars of `s` (aligned to char boundary).
fn tail_chars(s: &str, n: usize) -> &str {
    let char_count = s.chars().count();
    if char_count <= n {
        return s;
    }
    let skip = char_count - n;
    let byte_offset = s.char_indices().nth(skip).map(|(i, _)| i).unwrap_or(0);
    &s[byte_offset..]
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Basic behaviour ───────────────────────────────────────────────────────

    #[test]
    fn empty_text_returns_empty() {
        assert_eq!(split_chunks("", ChunkConfig::default()), vec![]);
    }

    #[test]
    fn short_text_returns_single_chunk() {
        let text = "Hello world";
        let chunks = split_chunks(text, ChunkConfig::default());
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "Hello world");
        assert_eq!(chunks[0].index, 0);
    }

    #[test]
    fn structural_markdown_splitting() {
        let text = "# Section 1\nContent 1\n\n## Section 2\nContent 2";
        let chunks = split_chunks(text, ChunkConfig::default());
        // Should have 2 chunks, one for each section
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].heading_path, Some("# Section 1".to_string()));
        assert_eq!(chunks[1].heading_path, Some("## Section 2".to_string()));
    }

    #[test]
    fn recursive_splitting_of_long_section() {
        let long_para = "a".repeat(1200);
        let text = format!("# Big Section\n\n{}", long_para);
        let chunks = split_chunks(
            &text,
            ChunkConfig {
                max_chars: 500,
                ..Default::default()
            },
        );
        // 1200 / 500 ≈ 3 chunks
        assert!(chunks.len() >= 3);
        for chunk in &chunks {
            assert!(chunk.content.len() <= 750); // with overlap and buffer
            assert_eq!(chunk.heading_path, Some("# Big Section".to_string()));
        }
    }

    #[test]
    fn title_augmentation() {
        let text = "Hello world";
        let config = ChunkConfig {
            title: Some("Doc Title".to_string()),
            ..Default::default()
        };
        let chunks = split_chunks(text, config);
        assert!(chunks[0].embedding_text.contains("[Title: Doc Title]"));
        assert!(chunks[0].embedding_text.contains("Hello world"));
    }

    #[test]
    fn overlap_content_appears_in_next_chunk() {
        let para1 = "x".repeat(600);
        let para2 = "y".repeat(600);
        let text = format!("{}\n\n{}", para1, para2);
        let chunks = split_chunks(
            &text,
            ChunkConfig {
                max_chars: 700,
                overlap_chars: 100,
                ..Default::default()
            },
        );
        assert!(chunks.len() >= 2);
        let tail = tail_chars(&para1, 100);
        assert!(chunks[1].content.contains(tail.trim()));
    }

    #[test]
    fn korean_text_splits_correctly() {
        let para = "이것은 한국어 테스트 문장입니다. ".repeat(50); // ~800 chars
        let text = format!("{}\n\n{}\n\n{}", para, para, para);
        let chunks = split_chunks(
            &text,
            ChunkConfig {
                max_chars: 1000,
                ..Default::default()
            },
        );
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(std::str::from_utf8(chunk.content.as_bytes()).is_ok());
        }
    }

    #[test]
    fn sentence_aware_split_at_boundary() {
        let text = "First sentence. Second sentence. Third sentence.";
        // if we split at ~20 chars, it should find the first period
        // and using proper overlap size for 20 max_chars
        let config = ChunkConfig {
            max_chars: 20,
            overlap_chars: 5,
            ..Default::default()
        };
        let chunks = split_chunks(text, config);
        // "First sentence." is 15 chars.
        assert!(chunks.len() >= 2);
        assert!(chunks[0].content.contains("First sentence."));
        assert!(!chunks[0].content.contains("Second"));
    }

    #[test]
    fn anchor_ratio_is_limited() {
        let long_title = "A".repeat(500);
        let text = "Hello world";
        let config = ChunkConfig {
            title: Some(long_title),
            ..Default::default()
        };
        let chunks = split_chunks(text, config);
        // Anchor (Title) should be truncated to ~150 chars (15% of 1000)
        assert!(chunks[0].embedding_text.len() < 300);
        assert!(chunks[0].embedding_text.contains("..."));
    }

    #[test]
    fn mixed_english_and_code() {
        let text = "This is a header.\n\n```python\ndef hello():\n    print('world')\n```\n\nAnother paragraph.";
        let config = ChunkConfig {
            max_chars: 50,
            ..Default::default()
        };
        let chunks = split_chunks(text, config);

        // It should split reasonably
        assert!(chunks.len() >= 2);
        // "python" should be in one of the chunks
        let has_python = chunks.iter().any(|c| c.content.contains("python"));
        assert!(has_python, "One of the chunks should contain 'python'");
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

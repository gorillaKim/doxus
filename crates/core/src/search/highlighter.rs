use aho_corasick::{AhoCorasick, MatchKind};
use anyhow::Result;
use memmap2::Mmap;
use std::fs::File;
use std::path::Path;

/// 고성능 하이라이터: 원본 파일 또는 텍스트에서 검색어 조각을 추출하고 하이라이팅 함.
pub struct HighlightingResult {
    pub snippet: String,
    pub matches_found: usize,
}

pub struct Highlighter {
    ac: AhoCorasick,
}

impl Highlighter {
    pub fn new(keywords: &[String]) -> Result<Self> {
        let ac = AhoCorasick::builder()
            .match_kind(MatchKind::LeftmostFirst)
            .ascii_case_insensitive(true)
            .build(keywords)
            .map_err(|e| anyhow::anyhow!("failed to build AhoCorasick: {}", e))?;

        Ok(Self { ac })
    }

    /// 파일에서 특정 바이트 범위를 읽어 하이라이팅된 스니펫을 생성 (Reference mode 전용)
    pub fn highlight_file(
        &self,
        path: &Path,
        start_byte: usize,
        end_byte: usize,
        context_padding: usize,
    ) -> Result<HighlightingResult> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };

        // 1. 컨텍스트를 고려한 범위 설정 (Padding)
        let total_size = mmap.len();
        let safe_start = start_byte.saturating_sub(context_padding);
        let safe_end = (end_byte + context_padding).min(total_size);

        // 2. 유니코드 경계 안전하게 조정 (UTF-8 깨짐 방지)
        let adjusted_start = self.adjust_to_unicode_boundary(&mmap, safe_start, true);
        let adjusted_end = self.adjust_to_unicode_boundary(&mmap, safe_end, false);

        let slice = &mmap[adjusted_start..adjusted_end];
        let text = String::from_utf8_lossy(slice);

        Ok(self.highlight_text(&text))
    }

    /// 텍스트에서 하이라이팅된 스니펫 생성 (Snapshot mode 및 일반 활용)
    pub fn highlight_text(&self, text: &str) -> HighlightingResult {
        let mut result = String::with_capacity(text.len() + 32);
        let mut last_match_end = 0;
        let mut matches_found = 0;

        for mat in self.ac.find_iter(text) {
            let start = mat.start();
            let end = mat.end();

            // 매치 이전 텍스트 추가
            result.push_str(&text[last_match_end..start]);

            // 하이라이트 태그 삽입
            result.push_str("<b>");
            result.push_str(&text[start..end]);
            result.push_str("</b>");

            last_match_end = end;
            matches_found += 1;
        }

        // 남은 텍스트 추가
        result.push_str(&text[last_match_end..]);

        HighlightingResult {
            snippet: result,
            matches_found,
        }
    }

    /// 유니코드 경계로 오프셋 조정
    fn adjust_to_unicode_boundary(&self, data: &[u8], mut offset: usize, forward: bool) -> usize {
        if offset == 0 || offset >= data.len() {
            return offset;
        }

        // UTF-8의 선행 바이트(leading byte)는 0b10xxxxxx 가 아님
        // 0b10xxxxxx 는 후속 바이트(continuation byte)임.
        if forward {
            while offset < data.len() && (data[offset] & 0xC0) == 0x80 {
                offset += 1;
            }
        } else {
            while offset > 0 && (data[offset] & 0xC0) == 0x80 {
                offset -= 1;
            }
        }
        offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_highlighting() {
        let hl = Highlighter::new(&vec!["rust".to_string(), "highlighter".to_string()]).unwrap();
        let res = hl.highlight_text("This is a rust highlighter test.");
        assert_eq!(
            res.snippet,
            "This is a <b>rust</b> <b>highlighter</b> test."
        );
        assert_eq!(res.matches_found, 2);
    }

    #[test]
    fn test_case_insensitive() {
        let hl = Highlighter::new(&vec!["RUST".to_string()]).unwrap();
        let res = hl.highlight_text("Rust programming");
        assert_eq!(res.snippet, "<b>Rust</b> programming");
    }

    #[test]
    fn test_unicode_safety() {
        let text = "안녕하세요, 고성능 하이라이터 테스트입니다.";
        let hl = Highlighter::new(&vec!["하이라이터".to_string()]).unwrap();
        let res = hl.highlight_text(text);
        assert!(res.snippet.contains("<b>하이라이터</b>"));
    }
}

-- V17: content_cache 테이블에 전체 문서 데이터를 저장하는 data_json 컬럼 추가
-- 기존 content 컬럼은 하위 호환성을 위해 유지하되, 신규 저장 시에는 data_json에 전체 객체를 직렬화하여 저장합니다.

ALTER TABLE content_cache ADD COLUMN data_json TEXT;

-- 기존 데이터 마이그레이션 (선택 사항: 기존 content를 바탕으로 최소한의 JSON 구조 생성 가능)
-- 여기서는 단순히 컬럼만 추가하고, 이후 set 호출 시 채워지도록 합니다.

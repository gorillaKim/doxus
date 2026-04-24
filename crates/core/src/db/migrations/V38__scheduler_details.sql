-- V38: 스케줄 상세 정보 및 제약 사항 추가
ALTER TABLE scheduled_jobs ADD COLUMN description TEXT;
ALTER TABLE scheduled_jobs ADD COLUMN is_immutable INTEGER NOT NULL DEFAULT 0;

-- 기존 시스템 스케줄을 불변으로 설정
UPDATE scheduled_jobs SET is_immutable = 1, description = '문서의 신선도 점수를 주기적으로 업데이트합니다.' WHERE job_name = 'Freshness Refresh';

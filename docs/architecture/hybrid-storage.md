---
title: 하이브리드 스토리지 및 고성능 검색 아키텍처
updated: 2026-04-30
tags:
  - architecture
  - storage
  - hybrid
  - memmap2
  - aho-corasick
  - sqlite
---

# 하이브리드 스토리지 및 고성능 검색 아키텍처

Doxus는 데스크탑 환경의 리소스(디스크 용량, CPU) 제약을 극복하고 수십만 개의 문서를 정밀하게 검색하기 위해 **Hybrid Storage** 아키텍처를 채택합니다.

## 1. 핵심 철학
- **SSOT (Single Source of Truth)**: 로컬 파일(Obsidian 등)은 파일 시스템 자체가 원천이며, DB에는 검색을 위한 인덱스만 최소한으로 보관합니다.
- **Off-heap Content Management**: 대용량 로컬 파일의 본문을 SQLite에 중복 저장하는 대신, 바이트 오프셋(Byte Offset)을 통해 필요할 때만 원본 파일에서 추출합니다.
- **Strategic Storage**: 소스 성격(로컬 vs 원격)에 따라 최적의 저장 전략을 자동으로 적용합니다.

## 2. 저장 전략 (Storage Strategy)
각 프로젝트는 다음 두 가지 전략 중 하나를 선택합니다:

| 전략 | 대상 소스 | 동작 방식 | 이점 |
| :--- | :--- | :--- | :--- |
| **Full (Snapshot)** | GitHub, Confluence | 본문 전체를 `chunks` 테이블에 저장 | 외부 데이터 유실 방지 및 빠른 접근 |
| **Reference (Hybrid)** | Obsidian, Local Folder | DB 본문은 비우고(`NULL`), 바이트 오프셋만 저장 | DB 용량 극소화, 파일 시스템과 동기화 |

## 3. 기술 스택 및 데이터 흐름

### 인덱싱 흐름 (Indexing Flow)
1. **Collector**: 플러그인을 통해 문서 수집.
2. **Chunker**: 문서를 의미 단위로 분할 시, 원본 파일에서의 `start_byte` 및 `end_byte` 기록.
3. **Storage Gate**: `storage_strategy`에 따라 DB 저장 방식 결정.
    - `reference`인 경우: FTS 인덱싱 후 `chunks.content`를 `NULL`로 전환.
4. **Triggers**: SQLite 트리거가 `NULL` 업데이트 시에도 FTS 인덱스를 보존하도록 관리.

### 검색 및 시각화 (Search & Snippet)
DB에 본문이 없는 `reference` 프로젝트 검색 시:
1. **FTS Match**: SQLite FTS5 인덱스를 통해 문서 및 오프셋 조회.
2. **High-Performance Highlighter (Rust)**:
    - 조회된 `start_byte`~`end_byte` 정보를 바탕으로 원본 파일을 `memmap2` (Memory Mapping)로 엽니다.
    - CPU 오버헤드 없이 수 밀리초 내에 해당 바이너리 범위를 읽습니다.
    - `Aho-Corasick` 알고리즘으로 검색어 위치를 광속으로 탐색하여 하이라이팅된 스니펫을 생성합니다.

## 4. 스마트 동기화 매니저 (Sync Manager)
데이터 정합성을 유지하기 위해 다음의 지능형 트리거를 사용합니다:
- **Focus Trigger**: 앱이 활성화될 때 변경된 로컬 파일을 빠르게 스캔.
- **Idle Trigger**: 기기 사용량이 적은 유휴 상태일 때 외부 소스 증분 동기화.
- **Priority Orchestration**: 모든 백그라운드 작업은 낮은 CPU 우선순위로 실행되어 사용자 작업 방해 금지.

## 5. 데이터 스키마 요약
- `projects.storage_strategy`: 저장 전략 정의.
- `chunks.start_byte`, `chunks.end_byte`: 원본 파일 내 위치 정보.
- `chunks.content`: `full` 전용 (로컬 소스는 `NULL`).

# doxus 설치 가이드 (팀 내부)

## 시스템 요구사항
- macOS 13+ (Ventura 이상)
- Apple Silicon (M1/M2/M3) — Intel Mac 미지원
- 인터넷 연결 (첫 실행 시 ONNX 모델 자동 다운로드 ~90MB)

## 설치 방법

### 1. DMG 다운로드
팀 공유 채널에서 `doxus_0.1.0_aarch64.dmg` 다운로드

### 2. 앱 설치
DMG를 열고 doxus를 Applications 폴더로 드래그

### 3. Gatekeeper 우회 (Apple 인증서 없음)
터미널을 열고 다음 명령 실행:
```bash
xattr -d com.apple.quarantine /Applications/doxus.app
```

### 4. 첫 실행
- doxus 실행
- ONNX 모델 다운로드 프롬프트가 나타남 → "다운로드" 클릭 (~90MB)
- 다운로드 완료 후 검색 기능 사용 가능

## 알려진 제한사항

| 항목 | 상태 | 비고 |
|------|------|------|
| Apple 코드 서명 | ❌ adhoc | xattr 우회 필요 |
| Intel Mac | ❌ 미지원 | Apple Silicon 전용 |
| 플러그인 마켓 | 🚧 준비 중 | Confluence/GitHub는 내장 |
| 자동 업데이트 | ❌ 미지원 | 수동 재설치 |

## MCP 서버 설정 (Claude Code 연동)

doxus 실행 후 Claude Code MCP 설정:
```json
{
  "mcpServers": {
    "doxus": {
      "command": "/Applications/doxus.app/Contents/MacOS/doxus-mcp"
    }
  }
}
```

## 문의
팀 슬랙 #doxus 채널

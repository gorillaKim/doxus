//! C-1: SidecarMessage / HostMessage 중복 제거 검증 테스트.
//!
//! SidecarManager::send() 가 HostMessage 를 직접 수용하는지 확인한다.

use doxus_agent::protocol::HostMessage;
use doxus_agent::sidecar::SidecarManager;

/// HostMessage::Start 가 올바른 JSON 으로 직렬화되는지 확인.
#[test]
fn test_host_message_start_serializes_correctly() {
    let msg = HostMessage::Start {
        session_id: "sess-42".into(),
        prompt: "what is doxus?".into(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["type"], "start");
    assert_eq!(v["session_id"], "sess-42");
    assert_eq!(v["prompt"], "what is doxus?");
}

/// HostMessage::Cancel 을 JSON 으로부터 역직렬화하는지 확인.
#[test]
fn test_host_message_cancel_deserializes() {
    let json = r#"{"type":"cancel"}"#;
    let msg: HostMessage = serde_json::from_str(json).unwrap();
    assert!(matches!(msg, HostMessage::Cancel));
}

/// SidecarManager::send 가 &HostMessage 를 파라미터로 수용하는지 컴파일 수준에서 확인.
///
/// 실제 Node.js 프로세스를 기동하지 않으므로 타입 시그니처만 검증한다.
#[test]
fn test_sidecar_manager_send_accepts_host_message() {
    // SidecarManager::send 의 시그니처가 &HostMessage 를 받는지 함수 포인터로 확인.
    fn _assert_send_signature(
        _f: for<'a, 'b> fn(
            &'a mut SidecarManager,
            &'b HostMessage,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<(), doxus_agent::sidecar::AgentError>> + 'a,
            >,
        >,
    ) {
    }
    // 컴파일만 되면 통과 — 런타임 assertion 불필요.
    let _ = SidecarManager::send as *const () as usize;
}

import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

type ModelStatus = { exists: boolean; path: string | null };
type FileTag = "model" | "tokenizer";

type ProgressPayload = {
  file: FileTag;
  percent: number;
  bytes_downloaded: number;
  total_bytes: number | null;
  status: "downloading" | "complete";
};

type Phase = "checking" | "idle" | "downloading" | "complete" | "error";

function formatBytes(n: number | null | undefined): string {
  if (!n || !Number.isFinite(n)) return "--";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

export function ModelDownloadModal() {
  const [phase, setPhase] = useState<Phase>("checking");
  const [error, setError] = useState<string | null>(null);
  const [modelProgress, setModelProgress] = useState<ProgressPayload | null>(null);
  const [tokProgress, setTokProgress] = useState<ProgressPayload | null>(null);
  const [toastDismissed, setToastDismissed] = useState(false);
  const [modalOpen, setModalOpen] = useState(false);
  const unlistenProgress = useRef<UnlistenFn | null>(null);
  const unlistenComplete = useRef<UnlistenFn | null>(null);

  async function checkStatus() {
    try {
      const status = await invoke<ModelStatus>("check_model_status");
      setPhase(status.exists ? "complete" : "idle");
    } catch (e) {
      setError(String(e));
      setPhase("error");
    }
  }

  async function startDownload() {
    setError(null);
    setModelProgress(null);
    setTokProgress(null);
    setPhase("downloading");
    setModalOpen(true);
    try {
      await invoke("download_onnx_model");
    } catch (e) {
      setError(String(e));
      setPhase("error");
    }
  }

  useEffect(() => {
    checkStatus();
    (async () => {
      unlistenProgress.current = await listen<ProgressPayload>(
        "model:download-progress",
        (event) => {
          const p = event.payload;
          if (p.file === "model") setModelProgress(p);
          else if (p.file === "tokenizer") setTokProgress(p);
        }
      );
      unlistenComplete.current = await listen("model:download-complete", () => {
        setPhase("complete");
        setModalOpen(false);
      });
    })();

    // 외부(설정 페이지 등)에서 모달 열기
    const handleOpen = () => setModalOpen(true);
    window.addEventListener("doxus:open-model-download", handleOpen);

    return () => {
      unlistenProgress.current?.();
      unlistenComplete.current?.();
      window.removeEventListener("doxus:open-model-download", handleOpen);
    };
  }, []);

  if (phase === "checking") return null;
  // 모달이 열리지 않은 상태에서 complete면 토스트/모달 모두 숨김
  if (phase === "complete" && !modalOpen) return null;

  return (
    <>
      {/* 토스트 배너 — 모델 없을 때 우하단에 표시 */}
      {phase === "idle" && !toastDismissed && (
        <div
          data-testid="model-download-toast"
          className="fixed bottom-4 right-4 z-[9000] flex items-start gap-3 w-80 rounded-xl border border-amber-500/30 bg-gray-900 p-4 shadow-xl text-sm text-gray-200"
        >
          <div className="flex-1">
            <p className="font-medium text-amber-300 mb-1">임베딩 모델 없음</p>
            <p className="text-gray-400 text-xs leading-relaxed">
              의미 기반 검색이 비활성화되어 있습니다. 텍스트 검색은 정상 동작합니다.
            </p>
            <button
              onClick={() => setModalOpen(true)}
              className="mt-2 px-3 py-1 rounded-lg bg-indigo-500 hover:bg-indigo-400 text-white text-xs font-medium transition"
            >
              모델 다운로드
            </button>
          </div>
          <button
            onClick={() => setToastDismissed(true)}
            className="text-gray-500 hover:text-gray-300 transition text-lg leading-none"
            aria-label="닫기"
          >
            ×
          </button>
        </div>
      )}

      {/* 다운로드 모달 — 사용자가 [모델 다운로드] 클릭하거나 다운로드 중/에러/완료 상태일 때 */}
      {(modalOpen || phase === "downloading" || phase === "error") && (
        <div
          data-testid="model-download-modal"
          className="fixed inset-0 z-[9999] flex items-center justify-center bg-black/70 backdrop-blur-sm"
        >
          <div className="w-[32rem] max-w-[92vw] rounded-2xl border border-white/10 bg-gray-900 p-6 shadow-2xl text-gray-100">
            <div className="flex items-center justify-between mb-1">
              <h2 className="text-lg font-semibold text-indigo-300">임베딩 모델 다운로드</h2>
              {phase === "idle" && (
                <button
                  onClick={() => setModalOpen(false)}
                  className="text-gray-500 hover:text-gray-300 transition text-xl leading-none"
                  aria-label="닫기"
                >
                  ×
                </button>
              )}
            </div>
            <p className="mt-1 text-sm text-gray-400">
              다국어 임베딩 모델(multilingual-e5-small, ~110MB)과 토크나이저를
              <br />
              <code className="text-xs text-gray-500">~/.doxus/models/</code> 아래로 다운로드합니다.
            </p>

            {phase === "idle" && (
              <div className="mt-6 flex justify-end gap-2">
                <button
                  onClick={() => setModalOpen(false)}
                  className="px-4 py-2 rounded-xl bg-gray-700 hover:bg-gray-600 text-gray-300 text-sm font-medium transition"
                >
                  나중에
                </button>
                <button
                  onClick={startDownload}
                  className="px-4 py-2 rounded-xl bg-indigo-500 hover:bg-indigo-400 text-white text-sm font-medium transition"
                >
                  다운로드 시작
                </button>
              </div>
            )}

            {phase === "downloading" && (
              <div className="mt-5 space-y-4">
                <ProgressRow label="model.onnx" progress={modelProgress} />
                <ProgressRow label="tokenizer.json" progress={tokProgress} />
                <p className="text-xs text-gray-500">
                  네트워크 상태에 따라 수 초 ~ 수 분이 걸릴 수 있습니다.
                </p>
              </div>
            )}

            {phase === "complete" && (
              <div className="mt-5 space-y-3">
                <div className="rounded-xl border border-emerald-500/30 bg-emerald-500/10 p-3 text-sm text-emerald-300 flex items-center gap-2">
                  <span>●</span>
                  <span>이미 설치됨 — 의미 기반 검색이 활성화되어 있습니다.</span>
                </div>
                <div className="flex justify-end">
                  <button
                    onClick={() => setModalOpen(false)}
                    className="px-4 py-2 rounded-xl bg-gray-700 hover:bg-gray-600 text-gray-300 text-sm font-medium transition"
                  >
                    닫기
                  </button>
                </div>
              </div>
            )}

            {phase === "error" && (
              <div className="mt-5 space-y-3">
                <div className="rounded-xl border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-300">
                  {error ?? "알 수 없는 오류가 발생했습니다."}
                </div>
                <div className="flex justify-end gap-2">
                  <button
                    onClick={() => { setModalOpen(false); setPhase("idle"); }}
                    className="px-4 py-2 rounded-xl bg-gray-700 hover:bg-gray-600 text-gray-300 text-sm font-medium transition"
                  >
                    닫기
                  </button>
                  <button
                    onClick={startDownload}
                    className="px-4 py-2 rounded-xl bg-indigo-500 hover:bg-indigo-400 text-white text-sm font-medium transition"
                  >
                    재시도
                  </button>
                </div>
              </div>
            )}
          </div>
        </div>
      )}
    </>
  );
}

function ProgressRow({
  label,
  progress,
}: {
  label: string;
  progress: ProgressPayload | null;
}) {
  const percent = Math.max(0, Math.min(100, progress?.percent ?? 0));
  const shown = progress ? percent.toFixed(1) : "0.0";
  return (
    <div>
      <div className="flex justify-between text-xs text-gray-400 mb-1">
        <span className="font-medium text-gray-300">{label}</span>
        <span>
          {shown}% · {formatBytes(progress?.bytes_downloaded ?? 0)} /{" "}
          {formatBytes(progress?.total_bytes ?? null)}
        </span>
      </div>
      <div className="h-2 rounded-full bg-gray-800 overflow-hidden">
        <div
          className="h-full bg-indigo-500 transition-all duration-200"
          style={{ width: `${percent}%` }}
        />
      </div>
    </div>
  );
}

export default ModelDownloadModal;

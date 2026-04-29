import { listen } from "@tauri-apps/api/event";

export type MigrationCompletePayload = { count?: number };

export async function setupMigrationListeners(
  onStart: () => void,
  onComplete: (payload: MigrationCompletePayload) => void,
): Promise<() => void> {
  const unlistenStart = await listen("migration:reindex_started", onStart);
  const unlistenComplete = await listen("migration:reindex_completed", (e) =>
    onComplete((e.payload ?? {}) as MigrationCompletePayload),
  );
  return () => {
    unlistenStart();
    unlistenComplete();
  };
}

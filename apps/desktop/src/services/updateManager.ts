import { check, type Update } from "@tauri-apps/plugin-updater";
import { emit } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

export interface UpdateInfo {
  version: string;
  notes: string | null;
  date: string | null;
}

// Discriminated union matching Tauri's DownloadEvent shape
type DownloadPhase = "Started" | "Progress" | "Finished";
export type ProgressPayload =
  | { event: "Started"; data: { contentLength: number | null } }
  | { event: "Progress"; data: { chunkLength: number } }
  | { event: "Finished" };

export interface NormalizedProgress {
  phase: DownloadPhase;
  percent: number;
  downloaded: number;
  total: number;
}

type ProgressCallback = (event: ProgressPayload) => void;

// Cache the resolved Update between checkForUpdates() and downloadAndInstall()
// so both functions see the same release and we avoid a second network round-trip.
let _pending: Update | null = null;

export interface CheckOptions {
  silent?: boolean;   // true → swallow errors (auto-check mode)
  timeoutMs?: number; // default 10 000 ms
}

/**
 * Check if a new version is available.
 * Emits `update:check` when the check starts, `update:available` when a new version is detected.
 * In `silent` mode, network/timeout errors return null instead of throwing.
 */
export async function checkForUpdates(opts: CheckOptions = {}): Promise<UpdateInfo | null> {
  const { silent = false, timeoutMs = 10_000 } = opts;

  await emit("update:check", {});

  try {
    const update = await Promise.race([
      check(),
      new Promise<never>((_, reject) =>
        setTimeout(() => reject(new Error("update check timed out")), timeoutMs),
      ),
    ]);

    _pending = update ?? null;

    if (!update) return null;

    await emit("update:available", {
      version: update.version,
      notes: update.body ?? null,
      date: update.date ?? null,
    });

    return {
      version: update.version,
      notes: update.body ?? null,
      date: update.date ?? null,
    };
  } catch (e) {
    if (silent) return null;
    throw e;
  }
}

/**
 * Download and install the latest available update.
 * Reuses the Update cached by the most recent `checkForUpdates()` call (no extra network request).
 * Falls back to a fresh `check()` if called standalone.
 * Emits `update:progress` with normalised 0–100 progress during download.
 * Emits `update:installed` after install completes.
 * On macOS, Tauri may terminate the app immediately after install — `update:installed`
 * is emitted *before* the final await resolves, from inside the Finished handler.
 */
export async function downloadAndInstall(onProgress?: ProgressCallback): Promise<void> {
  const update = _pending ?? (await check());
  _pending = null;

  if (!update) throw new Error("No update available");

  let total = 0;
  let downloaded = 0;
  let installedEmitted = false;

  await update.downloadAndInstall(async (raw: unknown) => {
    const event = raw as ProgressPayload;
    onProgress?.(event);

    if (event.event === "Started") {
      total = event.data.contentLength ?? 0;
    } else if (event.event === "Progress") {
      downloaded += event.data.chunkLength;
    } else if (event.event === "Finished") {
      // Emit before app may be terminated on macOS
      installedEmitted = true;
      try {
        await emit("update:installed", { version: update.version });
      } catch {
        // Window may already be closing — swallow
      }
    }

    const percent = total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : 0;
    try {
      await emit("update:progress", {
        phase: event.event,
        percent,
        downloaded,
        total,
      } satisfies NormalizedProgress);
    } catch {
      // Emit failure is non-fatal — install must continue
    }
  });

  if (!installedEmitted) {
    // Fallback for platforms where Finished fires after the promise resolves
    try {
      await emit("update:installed", { version: update.version });
    } catch {
      // Non-fatal — window may already be closing
    }
  }
}

/**
 * Relaunch the application into the newly installed version.
 * Delegates to the `relaunch_app` Tauri IPC command (implemented in update_manager.rs).
 */
export async function relaunchApp(): Promise<void> {
  await invoke("relaunch_app");
}

/** Visible for testing: clear the cached pending update. */
export function _clearPending(): void {
  _pending = null;
}

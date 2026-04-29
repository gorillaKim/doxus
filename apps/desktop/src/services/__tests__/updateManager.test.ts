import { describe, it, expect, vi, beforeEach } from "vitest";

const { mockCheck, mockEmit, mockInvoke } = vi.hoisted(() => ({
  mockCheck: vi.fn(),
  mockEmit: vi.fn(() => Promise.resolve()),
  mockInvoke: vi.fn(() => Promise.resolve()),
}));

vi.mock("@tauri-apps/plugin-updater", () => ({ check: mockCheck }));
vi.mock("@tauri-apps/api/event", () => ({ emit: mockEmit }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mockInvoke }));

import {
  checkForUpdates,
  downloadAndInstall,
  relaunchApp,
  _clearPending,
} from "../updateManager";

beforeEach(() => {
  vi.clearAllMocks();
  _clearPending();
});

// ── Helpers ──────────────────────────────────────────────────────────────────

function makeUpdate(overrides: Record<string, unknown> = {}) {
  return {
    version: "0.2.0",
    body: "Bug fixes and improvements",
    date: "2026-04-29",
    downloadAndInstall: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
}

// ── checkForUpdates ───────────────────────────────────────────────────────────

describe("checkForUpdates", () => {
  it("returns null when no update is available", async () => {
    mockCheck.mockResolvedValue(null);
    const result = await checkForUpdates();
    expect(result).toBeNull();
    expect(mockCheck).toHaveBeenCalledOnce();
  });

  it("returns UpdateInfo when update is available", async () => {
    mockCheck.mockResolvedValue(makeUpdate());
    const result = await checkForUpdates();
    expect(result?.version).toBe("0.2.0");
    expect(result?.notes).toBe("Bug fixes and improvements");
    expect(result?.date).toBe("2026-04-29");
  });

  it("normalizes missing body and date to null", async () => {
    mockCheck.mockResolvedValue(makeUpdate({ body: undefined, date: undefined }));
    const result = await checkForUpdates();
    expect(result?.notes).toBeNull();
    expect(result?.date).toBeNull();
  });

  it("emits update:check at start", async () => {
    mockCheck.mockResolvedValue(null);
    await checkForUpdates();
    expect(mockEmit).toHaveBeenCalledWith("update:check", {});
  });

  it("emits update:available when update is found", async () => {
    mockCheck.mockResolvedValue(makeUpdate());
    await checkForUpdates();
    expect(mockEmit).toHaveBeenCalledWith("update:available", {
      version: "0.2.0",
      notes: "Bug fixes and improvements",
      date: "2026-04-29",
    });
  });

  it("does not emit update:available when no update", async () => {
    mockCheck.mockResolvedValue(null);
    await checkForUpdates();
    expect(mockEmit).not.toHaveBeenCalledWith("update:available", expect.anything());
  });

  it("silently returns null on network error in silent mode", async () => {
    mockCheck.mockRejectedValue(new Error("network failure"));
    const result = await checkForUpdates({ silent: true });
    expect(result).toBeNull();
  });

  it("throws on network error in non-silent mode", async () => {
    mockCheck.mockRejectedValue(new Error("network failure"));
    await expect(checkForUpdates({ silent: false })).rejects.toThrow("network failure");
  });

  it("times out and throws after timeoutMs", async () => {
    mockCheck.mockImplementation(() => new Promise(() => {})); // never resolves
    await expect(checkForUpdates({ timeoutMs: 50 })).rejects.toThrow("update check timed out");
  }, 2000);

  it("times out silently when silent=true", async () => {
    mockCheck.mockImplementation(() => new Promise(() => {}));
    await expect(checkForUpdates({ silent: true, timeoutMs: 50 })).resolves.toBeNull();
  }, 2000);
});

// ── downloadAndInstall ────────────────────────────────────────────────────────

describe("downloadAndInstall", () => {
  it("throws when no update is available", async () => {
    mockCheck.mockResolvedValue(null);
    await expect(downloadAndInstall()).rejects.toThrow("No update available");
  });

  it("reuses cached update from checkForUpdates (no second check() call)", async () => {
    mockCheck.mockResolvedValue(makeUpdate());
    await checkForUpdates();
    mockCheck.mockClear();

    await downloadAndInstall();
    expect(mockCheck).not.toHaveBeenCalled();
  });

  it("calls check() if no cached update", async () => {
    mockCheck.mockResolvedValue(makeUpdate());
    await downloadAndInstall();
    expect(mockCheck).toHaveBeenCalledOnce();
  });

  it("forwards raw events to onProgress callback", async () => {
    const mockDlAndInstall = vi.fn().mockImplementation(async (cb: (e: unknown) => void) => {
      await cb({ event: "Started", data: { contentLength: 1000 } });
      await cb({ event: "Progress", data: { chunkLength: 500 } });
      await cb({ event: "Finished" });
    });
    mockCheck.mockResolvedValue(makeUpdate({ downloadAndInstall: mockDlAndInstall }));

    const onProgress = vi.fn();
    await downloadAndInstall(onProgress);
    expect(onProgress).toHaveBeenCalledTimes(3);
  });

  it("emits update:progress with normalized percent", async () => {
    const mockDlAndInstall = vi.fn().mockImplementation(async (cb: (e: unknown) => void) => {
      await cb({ event: "Started", data: { contentLength: 1000 } });
      await cb({ event: "Progress", data: { chunkLength: 500 } });
    });
    mockCheck.mockResolvedValue(makeUpdate({ downloadAndInstall: mockDlAndInstall }));

    await downloadAndInstall();
    expect(mockEmit).toHaveBeenCalledWith("update:progress", expect.objectContaining({
      percent: 50,
      downloaded: 500,
      total: 1000,
    }));
  });

  it("emits update:installed after install", async () => {
    mockCheck.mockResolvedValue(makeUpdate());
    await downloadAndInstall();
    expect(mockEmit).toHaveBeenCalledWith("update:installed", { version: "0.2.0" });
  });

  it("does not abort install when emit fails mid-download", async () => {
    const mockDlAndInstall = vi.fn().mockImplementation(async (cb: (e: unknown) => void) => {
      await cb({ event: "Progress", data: { chunkLength: 100 } });
    });
    mockCheck.mockResolvedValue(makeUpdate({ downloadAndInstall: mockDlAndInstall }));

    // Make emit reject on every call
    mockEmit.mockRejectedValue(new Error("channel closed"));

    // Should not throw — emit failure is non-fatal
    await expect(downloadAndInstall()).resolves.toBeUndefined();
  });
});

// ── relaunchApp ───────────────────────────────────────────────────────────────

describe("relaunchApp", () => {
  it("invokes the relaunch_app Tauri command", async () => {
    await relaunchApp();
    expect(mockInvoke).toHaveBeenCalledWith("relaunch_app");
  });
});

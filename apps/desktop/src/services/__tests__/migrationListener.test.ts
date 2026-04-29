import { describe, it, expect, vi, beforeEach } from "vitest";

const { mockListen } = vi.hoisted(() => ({
  mockListen: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: mockListen }));

import { setupMigrationListeners } from "../migrationListener";

type Handler = (e: unknown) => void;

describe("setupMigrationListeners", () => {
  let capturedHandlers: Record<string, Handler>;
  let unlistenFns: ReturnType<typeof vi.fn>[];

  beforeEach(() => {
    vi.clearAllMocks();
    capturedHandlers = {};
    unlistenFns = [];
    mockListen.mockImplementation((event: string, handler: Handler) => {
      capturedHandlers[event] = handler;
      const fn = vi.fn();
      unlistenFns.push(fn);
      return Promise.resolve(fn);
    });
  });

  it("subscribes to both migration events", async () => {
    await setupMigrationListeners(() => {}, () => {});
    expect(mockListen).toHaveBeenCalledWith("migration:reindex_started", expect.any(Function));
    expect(mockListen).toHaveBeenCalledWith("migration:reindex_completed", expect.any(Function));
  });

  it("calls onStart when migration:reindex_started fires", async () => {
    const onStart = vi.fn();
    await setupMigrationListeners(onStart, () => {});
    capturedHandlers["migration:reindex_started"]({});
    expect(onStart).toHaveBeenCalledOnce();
  });

  it("calls onComplete with payload when migration:reindex_completed fires", async () => {
    const onComplete = vi.fn();
    await setupMigrationListeners(() => {}, onComplete);
    capturedHandlers["migration:reindex_completed"]({ payload: { count: 42 } });
    expect(onComplete).toHaveBeenCalledWith({ count: 42 });
  });

  it("calls onComplete with empty payload when count is absent", async () => {
    const onComplete = vi.fn();
    await setupMigrationListeners(() => {}, onComplete);
    capturedHandlers["migration:reindex_completed"]({ payload: {} });
    expect(onComplete).toHaveBeenCalledWith({});
  });

  it("returns cleanup function that calls both unlisten fns", async () => {
    const cleanup = await setupMigrationListeners(() => {}, () => {});
    cleanup();
    expect(unlistenFns[0]).toHaveBeenCalledOnce();
    expect(unlistenFns[1]).toHaveBeenCalledOnce();
  });

  it("cleanup is idempotent — calling twice does not throw", async () => {
    const cleanup = await setupMigrationListeners(() => {}, () => {});
    expect(() => { cleanup(); cleanup(); }).not.toThrow();
  });

  it("rejects when the second listen() call fails", async () => {
    // First listen resolves, second rejects
    const firstUnlisten = vi.fn();
    let callCount = 0;
    mockListen.mockImplementation(() => {
      callCount++;
      if (callCount === 1) return Promise.resolve(firstUnlisten);
      return Promise.reject(new Error("event bus unavailable"));
    });
    await expect(setupMigrationListeners(() => {}, () => {})).rejects.toThrow(
      "event bus unavailable",
    );
  });
});

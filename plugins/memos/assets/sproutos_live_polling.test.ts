// @vitest-environment jsdom

import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invalidateQueries = vi.fn();

vi.mock("@tanstack/react-query", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@tanstack/react-query")>();
  return { ...actual, useQueryClient: () => ({ invalidateQueries }) };
});

vi.mock("@/contexts/AuthContext", () => ({
  useAuth: () => ({ currentUser: { name: "users/1" } }),
}));

vi.mock("@/connect", () => ({
  getRequestToken: async () => "test-token",
  refreshAccessToken: vi.fn(),
}));

import { SPROUTOS_LIVE_POLL_INTERVAL_MS, useLiveMemoRefresh } from "@/hooks/useLiveMemoRefresh";

function setVisibility(value: DocumentVisibilityState) {
  Object.defineProperty(document, "visibilityState", { configurable: true, value });
  document.dispatchEvent(new Event("visibilitychange"));
}

describe("SproutOS buffered-ingress live refresh", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    invalidateQueries.mockClear();
    Object.defineProperty(navigator, "onLine", { configurable: true, value: true });
    setVisibility("visible");
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("refreshes all live query families so remote create, update, and delete operations propagate", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    const { unmount } = renderHook(() => useLiveMemoRefresh());

    await vi.advanceTimersByTimeAsync(0);

    expect(invalidateQueries).toHaveBeenCalledTimes(4);
    for (const operation of ["create", "update", "delete"]) {
      await vi.advanceTimersByTimeAsync(SPROUTOS_LIVE_POLL_INTERVAL_MS);
      expect(invalidateQueries, operation).toHaveBeenCalledTimes(4 * (["create", "update", "delete"].indexOf(operation) + 2));
    }
    expect(fetchSpy).not.toHaveBeenCalled();
    unmount();
  });

  it("pauses after a background grace period and refreshes immediately on foreground resume", async () => {
    const { unmount } = renderHook(() => useLiveMemoRefresh());
    setVisibility("hidden");
    await vi.advanceTimersByTimeAsync(30_000);
    const pausedAt = invalidateQueries.mock.calls.length;

    await vi.advanceTimersByTimeAsync(SPROUTOS_LIVE_POLL_INTERVAL_MS * 2);
    expect(invalidateQueries).toHaveBeenCalledTimes(pausedAt);

    setVisibility("visible");
    await vi.advanceTimersByTimeAsync(0);
    // Reconnection invalidates once for potentially missed background changes, then the
    // immediate polling pulse runs through the normal change-event path.
    expect(invalidateQueries).toHaveBeenCalledTimes(pausedAt + 8);
    unmount();
  });
});

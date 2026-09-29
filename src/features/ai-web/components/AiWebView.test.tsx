// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AiWebView } from "./AiWebView";

const mocks = vi.hoisted(() => {
  const webviews = new Map<string, {
    label: string;
    show: ReturnType<typeof vi.fn>;
    hide: ReturnType<typeof vi.fn>;
    close: ReturnType<typeof vi.fn>;
  }>();
  return {
    webviews,
    createSidebarBrowser: vi.fn(async (label: string) => {
      webviews.set(label, {
        label,
        show: vi.fn(async () => {}),
        hide: vi.fn(async () => {}),
        close: vi.fn(async () => { webviews.delete(label); }),
      });
    }),
    setSidebarBrowserBounds: vi.fn(async () => {}),
    setSidebarBrowserVisible: vi.fn(async () => {}),
  };
});

vi.mock("@tauri-apps/api/webview", () => ({
  Webview: {
    getByLabel: vi.fn(async (label: string) => mocks.webviews.get(label) ?? null),
  },
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(async () => {}) }));
vi.mock("@services/events", () => ({ subscribeSidebarBrowserDownload: () => () => {} }));
vi.mock("@services/tauri", () => ({
  bridgeImportFile: vi.fn(),
  createSidebarBrowser: mocks.createSidebarBrowser,
  reloadSidebarBrowser: vi.fn(async () => {}),
  setSidebarBrowserBounds: mocks.setSidebarBrowserBounds,
  setSidebarBrowserVisible: mocks.setSidebarBrowserVisible,
}));

afterEach(() => cleanup());

describe("AI assistant browser tabs", () => {
  it("opens, switches, and closes independent tabs for the same assistant", async () => {
    render(<AiWebView />);

    fireEvent.click(screen.getByRole("button", { name: "Open ChatGPT" }));
    await waitFor(() => expect(mocks.createSidebarBrowser).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByRole("button", { name: "New assistant tab" }));
    fireEvent.click(screen.getByRole("button", { name: "Open ChatGPT" }));
    await waitFor(() => expect(mocks.createSidebarBrowser).toHaveBeenCalledTimes(2));

    const labels = mocks.createSidebarBrowser.mock.calls.map(([label]) => label);
    expect(new Set(labels).size).toBe(2);
    expect(screen.getAllByRole("tab", { name: "ChatGPT" })).toHaveLength(2);

    fireEvent.click(screen.getAllByRole("tab", { name: "ChatGPT" })[0]);
    await waitFor(() => expect(mocks.setSidebarBrowserVisible).toHaveBeenCalledWith(labels[0], true));

    fireEvent.click(screen.getAllByRole("button", { name: "Close ChatGPT tab" })[0]);
    await waitFor(() => expect(screen.getAllByRole("tab", { name: "ChatGPT" })).toHaveLength(1));
    expect(mocks.webviews.get(labels[0])).toBeUndefined();
  });
});

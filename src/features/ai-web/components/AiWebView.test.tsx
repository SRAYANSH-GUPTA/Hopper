// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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
    fileImported: new Set<(event: { webviewLabel: string; fileName: string; path: string; mimeType: string | null }) => void>(),
    downloadFinished: new Set<(event: { webviewLabel: string; fileName: string; path: string }) => void>(),
    offerComposerFile: vi.fn(),
  };
});

vi.mock("@tauri-apps/api/webview", () => ({
  Webview: {
    getByLabel: vi.fn(async (label: string) => mocks.webviews.get(label) ?? null),
  },
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(async () => {}) }));
vi.mock("@services/events", () => ({
  subscribeWebChatFileImported: (listener: Parameters<typeof mocks.fileImported.add>[0]) => {
    mocks.fileImported.add(listener);
    return () => mocks.fileImported.delete(listener);
  },
  subscribeWebChatDownloadFinished: (listener: Parameters<typeof mocks.downloadFinished.add>[0]) => {
    mocks.downloadFinished.add(listener);
    return () => mocks.downloadFinished.delete(listener);
  },
}));
vi.mock("@/features/web-chat/webChatFiles", () => ({
  offerComposerFile: mocks.offerComposerFile,
}));
vi.mock("@services/tauri", () => ({
  createSidebarBrowser: mocks.createSidebarBrowser,
  reloadSidebarBrowser: vi.fn(async () => {}),
  setSidebarBrowserBounds: mocks.setSidebarBrowserBounds,
  setSidebarBrowserVisible: mocks.setSidebarBrowserVisible,
}));

afterEach(() => cleanup());
beforeEach(() => vi.clearAllMocks());

describe("AI assistant browser tabs", () => {
  it("opens web search in its own browser tab", async () => {
    render(<AiWebView />);

    fireEvent.click(screen.getByRole("button", { name: "Open Web search" }));

    await waitFor(() => expect(mocks.createSidebarBrowser).toHaveBeenCalledWith(
      expect.any(String),
      "https://www.google.com",
      expect.any(Object),
    ));
    expect(screen.getByRole("tab", { name: "Web search" })).toBeTruthy();
    expect(screen.getByText("Google Search")).toBeTruthy();
    await waitFor(() => expect(screen.queryByText("Opening Web search…")).toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "Close Web search tab" }));
  });

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

  it("transfers chats between an assistant tab and Hopper with one click", async () => {
    vi.resetModules();
    const { AiWebView: FreshAiWebView } = await import("./AiWebView");
    const transfer = {
      sendToHopper: vi.fn(async () => "12 messages from ChatGPT will be sent with your next Hopper message."),
      pasteHopperChat: vi.fn(async () => {
        throw new Error("The active Hopper chat has no messages yet.");
      }),
    };
    render(<FreshAiWebView transfer={transfer} />);

    fireEvent.click(screen.getByRole("button", { name: "Open ChatGPT" }));
    const send = await screen.findByRole("button", { name: "Send chat to Hopper" });
    const label = mocks.createSidebarBrowser.mock.calls[0][0];

    fireEvent.click(send);
    await waitFor(() => expect(transfer.sendToHopper).toHaveBeenCalledWith(label));
    expect(await screen.findByText("12 messages from ChatGPT will be sent with your next Hopper message.")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Paste Hopper chat here" }));
    await waitFor(() => expect(transfer.pasteHopperChat).toHaveBeenCalledWith(label));
    expect(await screen.findByText("The active Hopper chat has no messages yet.")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Dismiss notice" }));
    expect(screen.queryByText("The active Hopper chat has no messages yet.")).toBeNull();
  });

  it("hides transfer actions on web search tabs", async () => {
    vi.resetModules();
    const { AiWebView: FreshAiWebView } = await import("./AiWebView");
    const transfer = { sendToHopper: vi.fn(), pasteHopperChat: vi.fn() };
    render(<FreshAiWebView transfer={transfer} />);

    fireEvent.click(screen.getByRole("button", { name: "Open Web search" }));
    await waitFor(() => expect(mocks.createSidebarBrowser).toHaveBeenCalled());
    await waitFor(() => expect(screen.queryByText("Opening Web search…")).toBeNull());
    expect(screen.queryByRole("button", { name: "Send chat to Hopper" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Paste Hopper chat here" })).toBeNull();
  });

  it("offers finished downloads and confirms files sent from the page", async () => {
    vi.resetModules();
    const { AiWebView: FreshAiWebView } = await import("./AiWebView");
    const { act } = await import("@testing-library/react");
    render(<FreshAiWebView />);

    fireEvent.click(screen.getByRole("button", { name: "Open Claude" }));
    await waitFor(() => expect(mocks.createSidebarBrowser).toHaveBeenCalled());
    const label = mocks.createSidebarBrowser.mock.calls[0][0];
    await waitFor(() => expect(screen.queryByText("Opening Claude…")).toBeNull());

    act(() => {
      mocks.downloadFinished.forEach((listener) =>
        listener({ webviewLabel: label, path: "/home/me/Downloads/report.pdf", fileName: "report.pdf" }));
    });
    expect(await screen.findByText("Downloaded report.pdf.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Send to Hopper chat" }));
    expect(mocks.offerComposerFile).toHaveBeenCalledWith("/home/me/Downloads/report.pdf");
    expect(await screen.findByText("Added report.pdf to your Hopper chat.")).toBeTruthy();

    act(() => {
      mocks.fileImported.forEach((listener) =>
        listener({ webviewLabel: label, path: "/data/x-chart.png", fileName: "chart.png", mimeType: "image/png" }));
    });
    expect(await screen.findByText("Added chart.png to your Hopper chat.")).toBeTruthy();

    act(() => {
      mocks.downloadFinished.forEach((listener) =>
        listener({ webviewLabel: "ai-chatbot-unknown", path: "/tmp/other.pdf", fileName: "other.pdf" }));
    });
    expect(screen.queryByText("Downloaded other.pdf.")).toBeNull();
  });
});

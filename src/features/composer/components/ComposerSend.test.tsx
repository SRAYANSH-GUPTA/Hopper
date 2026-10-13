/** @vitest-environment jsdom */
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useRef, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { isMobilePlatform } from "../../../utils/platformPaths";
import { Composer } from "./Composer";
import type {
  AppOption,
  AppMention,
  ComposerSendIntent,
  FollowUpMessageBehavior,
  SendMessageResult,
} from "../../../types";

vi.mock("../../../services/dragDrop", () => ({
  subscribeWindowDragDrop: vi.fn(() => () => {}),
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => `tauri://${path}`,
}));

vi.mock("../../../utils/platformPaths", async () => {
  const actual = await vi.importActual<typeof import("../../../utils/platformPaths")>(
    "../../../utils/platformPaths",
  );
  return {
    ...actual,
    isMobilePlatform: vi.fn(() => false),
  };
});

type HarnessProps = {
  onSend: (
    text: string,
    images: string[],
    appMentions?: AppMention[],
    submitIntent?: ComposerSendIntent,
  ) => void | Promise<void | SendMessageResult>;
  apps?: AppOption[];
  isProcessing?: boolean;
  followUpMessageBehavior?: FollowUpMessageBehavior;
  steerAvailable?: boolean;
  selectedServiceTier?: "fast" | "flex" | null;
  attachments?: string[];
  draftKey?: string | null;
  historyKey?: string;
  externalDraft?: string;
  onAttachImages?: (images: string[]) => void;
};

function ComposerHarness({
  onSend,
  apps = [],
  isProcessing = false,
  followUpMessageBehavior = "queue",
  steerAvailable = false,
  selectedServiceTier = null,
  attachments = [],
  draftKey = "thread-1",
  historyKey = "workspace-1",
  externalDraft,
  onAttachImages,
}: HarnessProps) {
  const [draftText, setDraftText] = useState("");
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

  return (
    <Composer
      onSend={onSend}
      onStop={() => {}}
      canStop={false}
      isProcessing={isProcessing}
      appsEnabled={true}
      steerAvailable={steerAvailable}
      followUpMessageBehavior={followUpMessageBehavior}
      composerFollowUpHintEnabled={true}
      collaborationModes={[]}
      selectedCollaborationModeId={null}
      onSelectCollaborationMode={() => {}}
      models={[]}
      selectedModelId={null}
      onSelectModel={() => {}}
      reasoningOptions={[]}
      selectedEffort={null}
      onSelectEffort={() => {}}
      selectedServiceTier={selectedServiceTier}
      reasoningSupported={false}
      accessMode="current"
      onSelectAccessMode={() => {}}
      skills={[]}
      apps={apps}
      prompts={[]}
      files={[]}
      draftText={externalDraft ?? draftText}
      draftKey={draftKey}
      historyKey={historyKey}
      onDraftChange={setDraftText}
      textareaRef={textareaRef}
      dictationEnabled={false}
      attachedImages={attachments}
      onAttachImages={onAttachImages}
    />
  );
}

describe("Composer send triggers", () => {
  afterEach(() => {
    cleanup();
    vi.mocked(isMobilePlatform).mockReturnValue(false);
    vi.restoreAllMocks();
  });

  it("keeps an unaccepted draft and prevents duplicate submits while sending", async () => {
    let finish!: (result: SendMessageResult) => void;
    const onSend = vi.fn(() => new Promise<SendMessageResult>((resolve) => { finish = resolve; }));
    render(<ComposerHarness onSend={onSend} />);
    const textarea = screen.getByRole("textbox") as HTMLTextAreaElement;

    fireEvent.change(textarea, { target: { value: "Keep my draft" } });
    fireEvent.keyDown(textarea, { key: "Enter" });
    fireEvent.keyDown(textarea, { key: "Enter" });
    expect(onSend).toHaveBeenCalledOnce();
    expect(textarea.value).toBe("Keep my draft");

    await act(async () => { finish({ status: "blocked" }); });

    expect(textarea.value).toBe("Keep my draft");
    fireEvent.keyDown(textarea, { key: "Enter" });
    expect(onSend).toHaveBeenCalledTimes(2);
    await act(async () => { finish({ status: "sent" }); });
    expect(textarea.value).toBe("");
  });

  it("preserves text edited while an earlier send is pending", async () => {
    let finish!: (result: SendMessageResult) => void;
    const onSend = vi.fn(() => new Promise<SendMessageResult>((resolve) => { finish = resolve; }));
    render(<ComposerHarness onSend={onSend} />);
    const textarea = screen.getByRole("textbox") as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: "First message" } });
    fireEvent.keyDown(textarea, { key: "Enter" });
    fireEvent.change(textarea, { target: { value: "Next message" } });

    await act(async () => { finish({ status: "sent" }); });

    expect(textarea.value).toBe("Next message");
  });

  it("retains a draft when its send rejects", async () => {
    const onSend = vi.fn().mockRejectedValue(new Error("Connection failed"));
    render(<ComposerHarness onSend={onSend} />);
    const textarea = screen.getByRole("textbox") as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: "Try this again" } });

    await act(async () => { fireEvent.keyDown(textarea, { key: "Enter" }); });

    expect(textarea.value).toBe("Try this again");
  });

  it("does not clear an identical draft in another thread after sending", async () => {
    let finish!: (result: SendMessageResult) => void;
    const onSend = vi.fn(() => new Promise<SendMessageResult>((resolve) => { finish = resolve; }));
    const { rerender } = render(<ComposerHarness onSend={onSend} draftKey="thread-1" externalDraft="Shared wording" />);
    const textarea = screen.getByRole("textbox") as HTMLTextAreaElement;
    fireEvent.keyDown(textarea, { key: "Enter" });
    rerender(<ComposerHarness onSend={onSend} draftKey="thread-2" externalDraft="Shared wording" />);

    await act(async () => { finish({ status: "sent" }); });

    expect(textarea.value).toBe("Shared wording");
  });

  it("restores text and attachments when a newly created thread rejects its first message", async () => {
    let finish!: (result: SendMessageResult) => void;
    const onSend = vi.fn(() => new Promise<SendMessageResult>((resolve) => { finish = resolve; }));
    const onAttachImages = vi.fn();
    const { rerender } = render(<ComposerHarness onSend={onSend} draftKey={null} externalDraft="First message" attachments={["/tmp/image.png"]} />);
    const textarea = screen.getByRole("textbox") as HTMLTextAreaElement;
    fireEvent.keyDown(textarea, { key: "Enter" });
    rerender(<ComposerHarness onSend={onSend} draftKey="new-thread" externalDraft="" onAttachImages={onAttachImages} />);

    await act(async () => { finish({ status: "blocked", threadId: "new-thread" }); });

    expect(textarea.value).toBe("First message");
    expect(onAttachImages).toHaveBeenCalledWith(["/tmp/image.png"]);
  });

  it("does not replace newer typing in a created thread when its first send fails", async () => {
    let finish!: (result: SendMessageResult) => void;
    const onSend = vi.fn(() => new Promise<SendMessageResult>((resolve) => { finish = resolve; }));
    const onAttachImages = vi.fn();
    const { rerender } = render(<ComposerHarness onSend={onSend} draftKey={null} externalDraft="First message" attachments={["/tmp/image.png"]} />);
    const textarea = screen.getByRole("textbox") as HTMLTextAreaElement;
    fireEvent.keyDown(textarea, { key: "Enter" });
    rerender(<ComposerHarness onSend={onSend} draftKey="new-thread" externalDraft="Another draft" onAttachImages={onAttachImages} />);

    await act(async () => { finish({ status: "blocked", threadId: "new-thread" }); });

    expect(textarea.value).toBe("Another draft");
    expect(onAttachImages).not.toHaveBeenCalled();
  });

  it("sends once on Enter", () => {
    const onSend = vi.fn();
    render(<ComposerHarness onSend={onSend} />);

    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "hello world" } });
    fireEvent.keyDown(textarea, { key: "Enter" });

    expect(onSend).toHaveBeenCalledTimes(1);
    expect(onSend).toHaveBeenCalledWith("hello world", [], undefined, "default");
  });

  it("sends once on send-button click", () => {
    const onSend = vi.fn();
    render(<ComposerHarness onSend={onSend} />);

    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "from button" } });
    fireEvent.click(screen.getByLabelText("Send"));

    expect(onSend).toHaveBeenCalledTimes(1);
    expect(onSend).toHaveBeenCalledWith("from button", [], undefined, "default");
  });

  it("sends document attachments as local file context", () => {
    const onSend = vi.fn();
    render(
      <ComposerHarness
        onSend={onSend}
        attachments={["/tmp/report.pdf", "/tmp/photo.png"]}
      />,
    );

    fireEvent.click(screen.getByLabelText("Send"));

    expect(onSend).toHaveBeenCalledWith(
      'Attached files (read these local paths before responding):\n- "/tmp/report.pdf"',
      ["/tmp/photo.png"],
      undefined,
      "default",
    );
  });

  it("shows the fast-mode indicator when enabled", () => {
    const onSend = vi.fn();
    render(<ComposerHarness onSend={onSend} selectedServiceTier="fast" />);

    expect(screen.getByLabelText("Fast mode enabled")).toBeTruthy();
  });

  it("blurs the textarea after Enter send on mobile", () => {
    vi.mocked(isMobilePlatform).mockReturnValue(true);
    const onSend = vi.fn();
    const blurSpy = vi.spyOn(HTMLTextAreaElement.prototype, "blur");
    render(<ComposerHarness onSend={onSend} />);

    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "dismiss keyboard" } });
    fireEvent.keyDown(textarea, { key: "Enter" });

    expect(onSend).toHaveBeenCalledTimes(1);
    expect(onSend).toHaveBeenCalledWith(
      "dismiss keyboard",
      [],
      undefined,
      "default",
    );
    expect(blurSpy).toHaveBeenCalledTimes(1);
  });

  it("sends explicit app mentions when an app autocomplete item is selected", () => {
    const onSend = vi.fn();
    render(
      <ComposerHarness
        onSend={onSend}
        apps={[
          {
            id: "connector_calendar",
            name: "Calendar App",
            description: "Calendar integration",
            isAccessible: true,
          },
        ]}
      />,
    );

    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "$cal" } });
    fireEvent.keyDown(textarea, { key: "Tab" });
    fireEvent.keyDown(textarea, { key: "Enter" });

    expect(onSend).toHaveBeenCalledTimes(1);
    expect(onSend).toHaveBeenCalledWith(
      "$calendar-app",
      [],
      [{ name: "Calendar App", path: "app://connector_calendar" }],
      "default",
    );
  });

  it("uses queue by default while processing when follow-up behavior is queue", () => {
    const onSend = vi.fn();
    render(
      <ComposerHarness
        onSend={onSend}
        isProcessing={true}
        followUpMessageBehavior="queue"
        steerAvailable={true}
      />,
    );

    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "queue this" } });
    fireEvent.keyDown(textarea, { key: "Enter" });

    expect(onSend).toHaveBeenCalledTimes(1);
    expect(onSend).toHaveBeenCalledWith("queue this", [], undefined, "queue");
  });

  it("uses opposite follow-up behavior on Shift+Ctrl+Enter while processing", () => {
    const onSend = vi.fn();
    render(
      <ComposerHarness
        onSend={onSend}
        isProcessing={true}
        followUpMessageBehavior="queue"
        steerAvailable={true}
      />,
    );

    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "steer this" } });
    fireEvent.keyDown(textarea, { key: "Enter", shiftKey: true, ctrlKey: true });

    expect(onSend).toHaveBeenCalledTimes(1);
    expect(onSend).toHaveBeenCalledWith("steer this", [], undefined, "steer");
  });

  it("falls back to queue when steer is selected but unavailable", () => {
    const onSend = vi.fn();
    render(
      <ComposerHarness
        onSend={onSend}
        isProcessing={true}
        followUpMessageBehavior="steer"
        steerAvailable={false}
      />,
    );

    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "queue fallback" } });
    fireEvent.keyDown(textarea, { key: "Enter" });

    expect(
      screen.getByText(
        "Default: Queue (Steer unavailable). Both Enter and Shift+Ctrl+Enter will queue this message.",
      ),
    ).toBeTruthy();
    expect(onSend).toHaveBeenCalledTimes(1);
    expect(onSend).toHaveBeenCalledWith("queue fallback", [], undefined, "queue");
  });

  it("treats Shift+Ctrl+Enter like normal send when not processing", () => {
    const onSend = vi.fn();
    render(
      <ComposerHarness
        onSend={onSend}
        isProcessing={false}
        followUpMessageBehavior="queue"
        steerAvailable={true}
      />,
    );

    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "normal shortcut send" } });
    fireEvent.keyDown(textarea, { key: "Enter", shiftKey: true, ctrlKey: true });

    expect(onSend).toHaveBeenCalledTimes(1);
    expect(onSend).toHaveBeenCalledWith(
      "normal shortcut send",
      [],
      undefined,
      "default",
    );
  });

  it("does not queue on Tab while processing", () => {
    const onSend = vi.fn();
    render(
      <ComposerHarness
        onSend={onSend}
        isProcessing={true}
        followUpMessageBehavior="queue"
        steerAvailable={true}
      />,
    );

    const textarea = screen.getByRole("textbox");
    fireEvent.change(textarea, { target: { value: "tab no send" } });
    fireEvent.keyDown(textarea, { key: "Tab" });

    expect(onSend).not.toHaveBeenCalled();
  });
});

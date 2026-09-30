// @vitest-environment jsdom
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useComposerDropSurface } from "@/features/composer/context/ComposerDropSurfaceContext";
import { ChatPane } from "./ChatPane";

function DropSurfaceProbe() {
  const surface = useComposerDropSurface();
  return <button onClick={() => surface?.setDragActive(true)}>Activate file drop</button>;
}

describe("ChatPane", () => {
  it("shows a pane-wide drop state for composer attachments", () => {
    const { container } = render(
      <ChatPane messagesNode={<div>Messages</div>} composerNode={<DropSurfaceProbe />} />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Activate file drop" }));

    expect(container.querySelector(".chat-pane")?.classList.contains("is-file-drag-over")).toBe(true);
  });
});

import { describe, expect, it, vi } from "vitest";
import { offerComposerFile, subscribeComposerFileOffers } from "./webChatFiles";

describe("webChatFiles", () => {
  it("delivers offered files to subscribers until they unsubscribe", () => {
    const listener = vi.fn();
    const unsubscribe = subscribeComposerFileOffers(listener);
    offerComposerFile("/tmp/report.pdf");
    unsubscribe();
    offerComposerFile("/tmp/ignored.pdf");
    expect(listener).toHaveBeenCalledTimes(1);
    expect(listener).toHaveBeenCalledWith("/tmp/report.pdf");
  });
});

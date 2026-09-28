import { describe, expect, it } from "vitest";
import {
  appendAttachedFileContext,
  isImageAttachment,
  splitComposerAttachments,
} from "./attachments";

describe("composer attachments", () => {
  it("recognizes local, remote, and pasted images", () => {
    expect(isImageAttachment("/tmp/photo.PNG")).toBe(true);
    expect(isImageAttachment("https://example.com/photo.jpg?size=2")).toBe(true);
    expect(isImageAttachment("data:image/png;base64,abc")).toBe(true);
    expect(isImageAttachment("/tmp/report.pdf")).toBe(false);
  });

  it("splits native images from readable local files", () => {
    expect(splitComposerAttachments(["/tmp/report.pdf", "/tmp/photo.png"])).toEqual({
      images: ["/tmp/photo.png"],
      files: ["/tmp/report.pdf"],
    });
  });

  it("appends file context to the user prompt", () => {
    expect(appendAttachedFileContext("Summarize this", ["/tmp/report.pdf"])).toBe(
      'Summarize this\n\nAttached files (read these local paths before responding):\n- "/tmp/report.pdf"',
    );
  });
});

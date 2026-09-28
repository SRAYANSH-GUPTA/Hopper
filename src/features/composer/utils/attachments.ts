const IMAGE_EXTENSIONS = [
  ".png",
  ".jpg",
  ".jpeg",
  ".gif",
  ".webp",
  ".bmp",
  ".tiff",
  ".tif",
  ".heic",
  ".heif",
];

export function isImageAttachment(value: string) {
  if (value.startsWith("data:image/")) {
    return true;
  }
  if (/^https?:\/\//i.test(value)) {
    try {
      return IMAGE_EXTENSIONS.some((extension) =>
        new URL(value).pathname.toLowerCase().endsWith(extension),
      );
    } catch {
      return false;
    }
  }
  const lower = value.toLowerCase();
  return IMAGE_EXTENSIONS.some((extension) => lower.endsWith(extension));
}

export function splitComposerAttachments(attachments: string[]) {
  const images: string[] = [];
  const files: string[] = [];
  for (const attachment of attachments) {
    (isImageAttachment(attachment) ? images : files).push(attachment);
  }
  return { images, files };
}

export function appendAttachedFileContext(text: string, files: string[]) {
  if (files.length === 0) {
    return text;
  }
  const fileList = files.map((path) => `- ${JSON.stringify(path)}`).join("\n");
  const context = `Attached files (read these local paths before responding):\n${fileList}`;
  return text.trim() ? `${text.trim()}\n\n${context}` : context;
}

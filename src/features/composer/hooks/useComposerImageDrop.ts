import { useCallback, useEffect, useRef, useState } from "react";
import { subscribeWindowDragDrop } from "../../../services/dragDrop";
import { isImageAttachment } from "../utils/attachments";
import { useComposerDropSurface } from "../context/ComposerDropSurfaceContext";

function isImageFile(file: File) {
  if (file.type.startsWith("image/")) {
    return true;
  }
  return isImageAttachment(file.name);
}

function getFilePath(file: File) {
  return (file as File & { path?: string }).path?.trim() ?? "";
}

function collectFilesFromTransfer(
  files: FileList | File[] | undefined,
  items: DataTransferItemList | DataTransferItem[] | undefined,
) {
  const transferFiles: File[] = [];
  if (files) {
    for (let i = 0; i < files.length; i++) {
      const file = files[i];
      if (file) {
        transferFiles.push(file);
      }
    }
  }

  const itemFiles: File[] = [];
  if (items) {
    for (let i = 0; i < items.length; i++) {
      const item = items[i];
      if (item && (item.kind === "file" || item.type?.startsWith("image/"))) {
        const file = item.getAsFile();
        if (file) {
          itemFiles.push(file);
        }
      }
    }
  }

  const uniqueFiles: File[] = [];
  const seen = new Set<string>();
  for (const file of [...transferFiles, ...itemFiles]) {
    const key = `${file.name}-${file.size}-${file.type}`;
    if (!seen.has(key)) {
      seen.add(key);
      uniqueFiles.push(file);
    }
  }
  return uniqueFiles;
}

function isDragFileTransfer(types: readonly string[] | undefined) {
  if (!types || types.length === 0) {
    return false;
  }
  return (
    types.includes("Files") ||
    types.includes("public.file-url") ||
    types.includes("application/x-moz-file")
  );
}

function readFilesAsDataUrls(files: File[]) {
  return Promise.all(
    files.map(
      (file) =>
        new Promise<string>((resolve) => {
          const reader = new FileReader();
          reader.onload = () =>
            resolve(typeof reader.result === "string" ? reader.result : "");
          reader.onerror = () => resolve("");
          reader.readAsDataURL(file);
        }),
    ),
  ).then((items) => items.filter(Boolean));
}

async function resolveAttachments(files: File[]) {
  const resolved = await Promise.all(
    files.map(async (file) => {
      const path = getFilePath(file);
      if (path) {
        return path;
      }
      if (!isImageFile(file)) {
        return "";
      }
      return (await readFilesAsDataUrls([file]))[0] ?? "";
    }),
  );
  return resolved.filter(Boolean);
}

function getDragPosition(position: { x: number; y: number }) {
  return position;
}

function normalizeDragPosition(
  position: { x: number; y: number },
  lastClientPosition: { x: number; y: number } | null,
) {
  const scale = window.devicePixelRatio || 1;
  if (scale === 1 || !lastClientPosition) {
    return getDragPosition(position);
  }
  const logicalDistance = Math.hypot(
    position.x - lastClientPosition.x,
    position.y - lastClientPosition.y,
  );
  const scaled = { x: position.x / scale, y: position.y / scale };
  const scaledDistance = Math.hypot(
    scaled.x - lastClientPosition.x,
    scaled.y - lastClientPosition.y,
  );
  return scaledDistance < logicalDistance ? scaled : position;
}

type UseComposerImageDropArgs = {
  disabled: boolean;
  onAttachImages?: (paths: string[]) => void;
};

export function useComposerImageDrop({
  disabled,
  onAttachImages,
}: UseComposerImageDropArgs) {
  const [isDragOver, setIsDragOver] = useState(false);
  const dropTargetRef = useRef<HTMLDivElement | null>(null);
  const lastClientPositionRef = useRef<{ x: number; y: number } | null>(null);
  const dropSurface = useComposerDropSurface();
  const setDragActive = useCallback((active: boolean) => {
    setIsDragOver(active);
    dropSurface?.setDragActive(active);
  }, [dropSurface]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    if (disabled) {
      return undefined;
    }
    unlisten = subscribeWindowDragDrop((event) => {
      const dropTarget = dropSurface?.targetRef.current ?? dropTargetRef.current;
      if (!dropTarget) {
        return;
      }
      if (event.payload.type === "leave") {
        setDragActive(false);
        return;
      }
      const position = normalizeDragPosition(
        event.payload.position,
        lastClientPositionRef.current,
      );
      const rect = dropTarget.getBoundingClientRect();
      const isInside =
        position.x >= rect.left &&
        position.x <= rect.right &&
        position.y >= rect.top &&
        position.y <= rect.bottom;
      if (event.payload.type === "over" || event.payload.type === "enter") {
        setDragActive(isInside);
        return;
      }
      if (event.payload.type === "drop") {
        setDragActive(false);
        if (!isInside) {
          return;
        }
        const attachmentPaths = (event.payload.paths ?? [])
          .map((path) => path.trim())
          .filter(Boolean);
        if (attachmentPaths.length > 0) {
          onAttachImages?.(attachmentPaths);
        }
      }
    });
    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, [disabled, dropSurface, onAttachImages, setDragActive]);

  const handleDragOver = (event: React.DragEvent<HTMLElement>) => {
    if (disabled) {
      return;
    }
    if (isDragFileTransfer(event.dataTransfer?.types)) {
      lastClientPositionRef.current = { x: event.clientX, y: event.clientY };
      event.preventDefault();
      setDragActive(true);
    }
  };

  const handleDragEnter = (event: React.DragEvent<HTMLElement>) => {
    handleDragOver(event);
  };

  const handleDragLeave = () => {
    if (isDragOver) {
      setDragActive(false);
      lastClientPositionRef.current = null;
    }
  };

  const handleDrop = async (event: React.DragEvent<HTMLElement>) => {
    if (disabled) {
      return;
    }
    event.preventDefault();
    setDragActive(false);
    lastClientPositionRef.current = null;
    const files = collectFilesFromTransfer(
      event.dataTransfer?.files,
      event.dataTransfer?.items,
    );
    if (files.length > 0) {
      const attachments = await resolveAttachments(files);
      if (attachments.length > 0) {
        onAttachImages?.(attachments);
      }
    }
  };

  const handlePaste = async (event: React.ClipboardEvent<HTMLTextAreaElement>) => {
    if (disabled) {
      return;
    }
    let files = collectFilesFromTransfer(
      event.clipboardData?.files,
      event.clipboardData?.items,
    );

    if (files.length === 0) {
      // Fallback: Webviews (like WebKitGTK in Tauri on Linux) sometimes do not populate
      // event.clipboardData.files/items synchronously for pasted images.
      if (
        typeof navigator !== "undefined" &&
        navigator.clipboard &&
        typeof navigator.clipboard.read === "function"
      ) {
        try {
          const clipboardItems = await navigator.clipboard.read();
          const clipboardFiles: File[] = [];
          for (const item of clipboardItems) {
            for (const type of item.types) {
              if (type.startsWith("image/")) {
                const blob = await item.getType(type);
                const file = new File([blob], "Pasted image", { type });
                clipboardFiles.push(file);
              }
            }
          }
          files = clipboardFiles.filter(isImageFile);
        } catch (err) {
          console.warn("Failed to read clipboard using navigator.clipboard.read:", err);
        }
      }
    }

    if (files.length === 0) {
      return;
    }

    event.preventDefault();
    const valid = await resolveAttachments(files);
    if (valid.length > 0) {
      onAttachImages?.(valid);
    }
  };

  return {
    dropTargetRef,
    isDragOver,
    handleDragOver,
    handleDragEnter,
    handleDragLeave,
    handleDrop,
    handlePaste,
  };
}

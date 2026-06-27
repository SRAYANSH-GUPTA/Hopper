import { useEffect, useRef, useState } from "react";
import { subscribeWindowDragDrop } from "../../../services/dragDrop";

const imageExtensions = [
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

function isImagePath(path: string) {
  const lower = path.toLowerCase();
  return imageExtensions.some((ext) => lower.endsWith(ext));
}

function isImageFile(file: File) {
  if (file.type.startsWith("image/")) {
    return true;
  }
  return isImagePath(file.name);
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

async function resolveImageAttachments(files: File[]) {
  const paths = files.map(getFilePath).filter(Boolean);
  if (paths.length > 0) {
    return paths;
  }
  return readFilesAsDataUrls(files);
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

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    if (disabled) {
      return undefined;
    }
    unlisten = subscribeWindowDragDrop((event) => {
      if (!dropTargetRef.current) {
        return;
      }
      if (event.payload.type === "leave") {
        setIsDragOver(false);
        return;
      }
      const position = normalizeDragPosition(
        event.payload.position,
        lastClientPositionRef.current,
      );
      const rect = dropTargetRef.current.getBoundingClientRect();
      const isInside =
        position.x >= rect.left &&
        position.x <= rect.right &&
        position.y >= rect.top &&
        position.y <= rect.bottom;
      if (event.payload.type === "over" || event.payload.type === "enter") {
        setIsDragOver(isInside);
        return;
      }
      if (event.payload.type === "drop") {
        setIsDragOver(false);
        if (!isInside) {
          return;
        }
        const imagePaths = (event.payload.paths ?? [])
          .map((path) => path.trim())
          .filter(Boolean)
          .filter(isImagePath);
        if (imagePaths.length > 0) {
          onAttachImages?.(imagePaths);
        }
      }
    });
    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, [disabled, onAttachImages]);

  const handleDragOver = (event: React.DragEvent<HTMLElement>) => {
    if (disabled) {
      return;
    }
    if (isDragFileTransfer(event.dataTransfer?.types)) {
      lastClientPositionRef.current = { x: event.clientX, y: event.clientY };
      event.preventDefault();
      setIsDragOver(true);
    }
  };

  const handleDragEnter = (event: React.DragEvent<HTMLElement>) => {
    handleDragOver(event);
  };

  const handleDragLeave = () => {
    if (isDragOver) {
      setIsDragOver(false);
      lastClientPositionRef.current = null;
    }
  };

  const handleDrop = async (event: React.DragEvent<HTMLElement>) => {
    if (disabled) {
      return;
    }
    event.preventDefault();
    setIsDragOver(false);
    lastClientPositionRef.current = null;
    const files = collectFilesFromTransfer(
      event.dataTransfer?.files,
      event.dataTransfer?.items,
    ).filter(isImageFile);
    if (files.length > 0) {
      onAttachImages?.(await resolveImageAttachments(files));
    }
  };

  const handlePaste = async (event: React.ClipboardEvent<HTMLTextAreaElement>) => {
    if (disabled) {
      return;
    }
    let files = collectFilesFromTransfer(
      event.clipboardData?.files,
      event.clipboardData?.items,
    ).filter(isImageFile);

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

    if (event.cancelable) {
      event.preventDefault();
    }
    const valid = await resolveImageAttachments(files);
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

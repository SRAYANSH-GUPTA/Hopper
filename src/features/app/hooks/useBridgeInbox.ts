import { useCallback, useEffect, useRef, useState } from "react";
import type { WorkspaceInfo } from "@/types";
import {
  bridgeGetImport, bridgeImportFile, bridgeListImports, bridgeMaterializeImport,
  bridgeReadArtifact, pickBridgeImportFile,
  type BridgeArtifactContent, type BridgeImport, type BridgeImportSummary,
} from "@services/tauri";
import { subscribeSidebarBrowserDownload } from "@services/events";
import {
  buildImportedHandoffPrompt,
  savePendingAttachments,
  savePendingHandoff,
} from "@/features/context/contextStore";

export type BridgeInboxProps = {
  workspaces: WorkspaceInfo[];
  activeWorkspaceId: string | null;
  activeThreadId: string | null;
  activeProviderLabel: string;
};

export function useBridgeInbox({
  workspaces, activeWorkspaceId, activeThreadId, activeProviderLabel,
}: BridgeInboxProps) {
  const [imports, setImports] = useState<BridgeImportSummary[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [selectedImport, setSelectedImport] = useState<BridgeImport | null>(null);
  const [artifactPath, setArtifactPath] = useState("");
  const [preview, setPreview] = useState<BridgeArtifactContent | null>(null);
  const [loading, setLoading] = useState(true);
  const [detailLoading, setDetailLoading] = useState(false);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [busy, setBusy] = useState<"import" | "attach" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const copiedPaths = useRef(new Map<string, string>());
  const listRequest = useRef(0);
  const actionPending = useRef(false);

  const refresh = useCallback(async () => {
    const request = ++listRequest.current;
    setLoading(true);
    try {
      const next = await bridgeListImports();
      if (request !== listRequest.current) return;
      setImports(next);
      setSelectedId((current) => next.some((item) => item.id === current) ? current : next[0]?.id ?? null);
    } catch (reason) {
      if (request === listRequest.current) setError(String(reason));
    } finally {
      if (request === listRequest.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    return () => { listRequest.current += 1; };
  }, [refresh]);

  useEffect(() => subscribeSidebarBrowserDownload((download) => {
    if (download.imported) void refresh();
  }), [refresh]);

  useEffect(() => {
    let cancelled = false;
    setSelectedImport(null);
    setArtifactPath("");
    setError(null);
    setNotice(null);
    setDetailLoading(Boolean(selectedId));
    if (selectedId) {
      void bridgeGetImport(selectedId).then((imported) => {
        if (cancelled) return;
        setSelectedImport(imported);
        setArtifactPath(imported.artifacts[0]?.path ?? "");
      }).catch((reason) => {
        if (!cancelled) setError(String(reason));
      }).finally(() => {
        if (!cancelled) setDetailLoading(false);
      });
    }
    return () => { cancelled = true; };
  }, [selectedId]);

  useEffect(() => {
    let cancelled = false;
    setPreview(null);
    setPreviewError(null);
    setPreviewLoading(Boolean(selectedImport && artifactPath));
    if (selectedImport && artifactPath) {
      void bridgeReadArtifact(selectedImport.id, artifactPath).then((content) => {
        if (!cancelled) setPreview(content);
      }).catch((reason) => {
        if (!cancelled) setPreviewError(String(reason));
      }).finally(() => {
        if (!cancelled) setPreviewLoading(false);
      });
    }
    return () => { cancelled = true; };
  }, [selectedImport, artifactPath]);

  const workspace = workspaces.find((item) => item.id === activeWorkspaceId);

  const runAction = async (action: "import" | "attach") => {
    if (actionPending.current) return;
    actionPending.current = true;
    setBusy(action);
    setError(null);
    setNotice(null);
    try {
      if (action === "import") {
        const path = await pickBridgeImportFile();
        if (!path) return;
        const imported = await bridgeImportFile(path);
        await refresh();
        setSelectedId(imported.id);
        return;
      }
      if (!workspace || !selectedImport) throw new Error("Open a chat in a workspace first.");
      const key = JSON.stringify([selectedImport.id, workspace.id]);
      let path = copiedPaths.current.get(key);
      if (!path) {
        const result = await bridgeMaterializeImport(selectedImport.id, workspace.id);
        path = result.path;
        copiedPaths.current.set(key, path);
      }
      const prompt = buildImportedHandoffPrompt({
        source: selectedImport.source,
        title: selectedImport.title,
        sourceUrl: selectedImport.sourceUrl,
        importedPath: path,
        messages: selectedImport.conversation.messages,
        artifacts: selectedImport.artifacts,
      }, activeProviderLabel);
      const artifactsDirectory = `${path.replace(/[\\/]$/, "")}/artifacts`;
      const attachmentPaths = selectedImport.artifacts.map(
        (artifact) => `${artifactsDirectory}/${artifact.path}`,
      );
      savePendingHandoff(workspace.id, prompt, activeThreadId);
      savePendingAttachments(workspace.id, attachmentPaths, activeThreadId);
      const fileLabel = attachmentPaths.length === 1 ? "file" : "files";
      setNotice(`${attachmentPaths.length} imported ${fileLabel} added to your chat in ${workspace.name}.`);
    } catch (reason) {
      setError(String(reason));
    } finally {
      actionPending.current = false;
      setBusy(null);
    }
  };

  return {
    imports, selectedId, setSelectedId, selectedImport,
    workspace, artifactPath, setArtifactPath,
    preview, loading, detailLoading, previewLoading, previewError, busy,
    error, setError, notice, refresh, runAction,
  };
}

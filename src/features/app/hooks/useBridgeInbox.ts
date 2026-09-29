import { useCallback, useEffect, useRef, useState } from "react";
import type { WorkspaceInfo } from "@/types";
import {
  bridgeGetImport, bridgeImportFile, bridgeListImports, bridgeMaterializeImport,
  bridgeReadArtifact, pickBridgeImportFile,
  type BridgeArtifactContent, type BridgeImport, type BridgeImportSummary,
} from "@services/tauri";
import { subscribeSidebarBrowserDownload } from "@services/events";
import { buildImportedHandoffPrompt, savePendingHandoff } from "@/features/context/contextStore";

export type BridgeInboxProps = {
  workspaces: WorkspaceInfo[];
  activeWorkspaceId: string | null;
  onSelectWorkspace: (workspaceId: string) => void;
  onProviderSwitch: (providerId: string) => void;
  onAddAgent: (workspace: WorkspaceInfo) => void;
};

export const BRIDGE_PROVIDERS = [
  { id: "codex", label: "Codex" },
  { id: "claude", label: "Claude Code" },
  { id: "antigravity", label: "Antigravity" },
];

export function useBridgeInbox({
  workspaces, activeWorkspaceId, onSelectWorkspace, onProviderSwitch, onAddAgent,
}: BridgeInboxProps) {
  const [imports, setImports] = useState<BridgeImportSummary[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [selectedImport, setSelectedImport] = useState<BridgeImport | null>(null);
  const [workspaceId, setWorkspaceId] = useState(activeWorkspaceId ?? "");
  const [providerId, setProviderId] = useState("codex");
  const [artifactPath, setArtifactPath] = useState("");
  const [preview, setPreview] = useState<BridgeArtifactContent | null>(null);
  const [loading, setLoading] = useState(true);
  const [detailLoading, setDetailLoading] = useState(false);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [busy, setBusy] = useState<"import" | "copy" | "start" | null>(null);
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

  useEffect(() => { setWorkspaceId(activeWorkspaceId ?? ""); }, [activeWorkspaceId]);

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

  const workspace = workspaces.find((item) => item.id === workspaceId);
  const provider = BRIDGE_PROVIDERS.find((item) => item.id === providerId) ?? BRIDGE_PROVIDERS[0];

  const runAction = async (action: "import" | "copy" | "start") => {
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
      if (!workspace || !selectedImport) throw new Error("Choose a workspace to continue.");
      const key = JSON.stringify([selectedImport.id, workspace.id]);
      let path = copiedPaths.current.get(key);
      if (!path) {
        const result = await bridgeMaterializeImport(selectedImport.id, workspace.id);
        path = result.path;
        copiedPaths.current.set(key, path);
      }
      if (action === "copy") {
        setNotice(`Files copied to ${path}`);
        return;
      }
      const prompt = buildImportedHandoffPrompt({
        source: selectedImport.source,
        title: selectedImport.title,
        sourceUrl: selectedImport.sourceUrl,
        importedPath: path,
        messages: selectedImport.conversation.messages,
        artifacts: selectedImport.artifacts,
      }, provider.label);
      onProviderSwitch(provider.id);
      savePendingHandoff(workspace.id, prompt);
      onSelectWorkspace(workspace.id);
      onAddAgent(workspace);
      setNotice(`Ready in ${provider.label}. The imported context will accompany your first message.`);
    } catch (reason) {
      setError(String(reason));
    } finally {
      actionPending.current = false;
      setBusy(null);
    }
  };

  return {
    imports, selectedId, setSelectedId, selectedImport, workspaceId, setWorkspaceId,
    workspace, providerId, setProviderId, provider, artifactPath, setArtifactPath,
    preview, loading, detailLoading, previewLoading, previewError, busy,
    error, setError, notice, refresh, runAction,
  };
}

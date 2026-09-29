import { useCallback, useEffect, useMemo, useState } from "react";
import ArrowRight from "lucide-react/dist/esm/icons/arrow-right";
import Archive from "lucide-react/dist/esm/icons/archive";
import Check from "lucide-react/dist/esm/icons/check";
import Download from "lucide-react/dist/esm/icons/download";
import FileText from "lucide-react/dist/esm/icons/file-text";
import LoaderCircle from "lucide-react/dist/esm/icons/loader-circle";
import RefreshCw from "lucide-react/dist/esm/icons/refresh-cw";
import ShieldCheck from "lucide-react/dist/esm/icons/shield-check";
import type { WorkspaceInfo } from "@/types";
import {
  bridgeGetImport,
  bridgeImportFile,
  bridgeListImports,
  bridgeMaterializeImport,
  bridgeReadArtifact,
  pickBridgeImportFile,
  type BridgeArtifactContent,
  type BridgeImport,
  type BridgeImportSummary,
} from "@services/tauri";
import { subscribeSidebarBrowserDownload } from "@services/events";
import {
  buildImportedHandoffPrompt,
  savePendingHandoff,
} from "@/features/context/contextStore";

type BridgeInboxProps = {
  workspaces: WorkspaceInfo[];
  activeWorkspaceId: string | null;
  onSelectWorkspace: (workspaceId: string) => void;
  onProviderSwitch: (providerId: string) => void;
  onAddAgent: (workspace: WorkspaceInfo) => void;
};

const PROVIDERS = [
  { id: "codex", label: "Codex" },
  { id: "claude", label: "Claude Code" },
  { id: "antigravity", label: "Antigravity" },
];

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function decodeText(content: BridgeArtifactContent): string | null {
  if (!content.mimeType?.startsWith("text/") && !/\.(md|txt|json|html|css|js|jsx|ts|tsx|svg)$/i.test(content.path)) {
    return null;
  }
  const binary = window.atob(content.contentBase64);
  const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

export function BridgeInbox({
  workspaces,
  activeWorkspaceId,
  onSelectWorkspace,
  onProviderSwitch,
  onAddAgent,
}: BridgeInboxProps) {
  const [imports, setImports] = useState<BridgeImportSummary[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [selectedImport, setSelectedImport] = useState<BridgeImport | null>(null);
  const [workspaceId, setWorkspaceId] = useState(activeWorkspaceId ?? "");
  const [materializedPath, setMaterializedPath] = useState<string | null>(null);
  const [artifactPreview, setArtifactPreview] = useState<BridgeArtifactContent | null>(null);
  const [targetProviderId, setTargetProviderId] = useState(PROVIDERS[0].id);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const next = await bridgeListImports();
    setImports(next);
    setSelectedId((current) => current ?? next[0]?.id ?? null);
  }, []);

  useEffect(() => {
    void refresh().catch((reason) => setError(String(reason)));
  }, [refresh]);

  useEffect(() => subscribeSidebarBrowserDownload((download) => {
    if (download.imported) {
      void refresh().catch((reason) => setError(String(reason)));
    }
  }), [refresh]);

  useEffect(() => {
    if (activeWorkspaceId) setWorkspaceId(activeWorkspaceId);
  }, [activeWorkspaceId]);

  useEffect(() => {
    setMaterializedPath(null);
  }, [workspaceId]);

  useEffect(() => {
    setArtifactPreview(null);
    setMaterializedPath(null);
    if (!selectedId) {
      setSelectedImport(null);
      return;
    }
    void bridgeGetImport(selectedId)
      .then(setSelectedImport)
      .catch((reason) => setError(String(reason)));
  }, [selectedId]);

  const selectedWorkspace = useMemo(
    () => workspaces.find((workspace) => workspace.id === workspaceId) ?? null,
    [workspaceId, workspaces],
  );
  const targetProvider = PROVIDERS.find((provider) => provider.id === targetProviderId) ?? PROVIDERS[0];

  const handleImport = async () => {
    const path = await pickBridgeImportFile();
    if (!path) return;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const imported = await bridgeImportFile(path);
      await refresh();
      setSelectedId(imported.id);
      setNotice("Imported into the Bridge Inbox. No workspace files were changed.");
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const materialize = async (): Promise<string> => {
    if (!selectedImport || !selectedWorkspace) {
      throw new Error("Choose a workspace before continuing.");
    }
    if (materializedPath) return materializedPath;
    const result = await bridgeMaterializeImport(selectedImport.id, selectedWorkspace.id);
    setMaterializedPath(result.path);
    return result.path;
  };

  const handleMaterialize = async () => {
    setBusy(true);
    setError(null);
    try {
      const path = await materialize();
      setNotice(`Copied the reviewed bundle to ${path}.`);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const handleContinue = async () => {
    setBusy(true);
    setError(null);
    try {
      const path = await materialize();
      if (!selectedImport || !selectedWorkspace) return;
      const prompt = buildImportedHandoffPrompt(
        {
          source: selectedImport.source,
          title: selectedImport.title,
          sourceUrl: selectedImport.sourceUrl,
          importedPath: path,
          messages: selectedImport.conversation.messages,
          artifacts: selectedImport.artifacts,
        },
        targetProvider.label,
      );
      onProviderSwitch(targetProvider.id);
      savePendingHandoff(selectedWorkspace.id, prompt);
      onSelectWorkspace(selectedWorkspace.id);
      onAddAgent(selectedWorkspace);
      setNotice(`Handoff prepared for ${targetProvider.label}. It will be attached to your next message.`);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const handlePreviewArtifact = async (path: string) => {
    if (!selectedImport) return;
    setBusy(true);
    setError(null);
    try {
      setArtifactPreview(await bridgeReadArtifact(selectedImport.id, path));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const previewText = artifactPreview ? decodeText(artifactPreview) : null;
  const previewImage = artifactPreview?.mimeType?.startsWith("image/")
    ? `data:${artifactPreview.mimeType};base64,${artifactPreview.contentBase64}`
    : null;

  return (
    <div className="bridge-inbox">
      <header className="bridge-header">
        <div className="bridge-title-lockup">
          <div className="bridge-mark"><Archive size={18} /></div>
          <div>
            <h2>Bridge inbox</h2>
            <p>Review assistant files before they enter your workspace.</p>
          </div>
        </div>
        <div className="bridge-header-actions">
          <button className="bridge-icon-button" type="button" onClick={() => void refresh()} aria-label="Refresh inbox" title="Refresh inbox"><RefreshCw size={15} /></button>
          <button className="bridge-primary-button" type="button" disabled={busy} onClick={() => void handleImport()}>
            {busy ? <LoaderCircle className="bridge-spin" size={15} /> : <Download size={15} />} Import file
          </button>
        </div>
      </header>

      {error && <div className="bridge-alert is-error">{error}</div>}
      {notice && <div className="bridge-alert is-success"><Check size={13} />{notice}</div>}

      <div className="bridge-layout">
        <aside className="bridge-queue">
          <div className="bridge-queue-heading">
            <span>Recent imports</span>
            <strong>{imports.length}</strong>
          </div>
          <nav className="bridge-import-list" aria-label="Bridge imports">
            {imports.length === 0 ? (
              <div className="bridge-empty">
                <div className="bridge-empty-icon"><Download size={18} /></div>
                <strong>No imports yet</strong>
                <span>Download from an assistant or choose a local file.</span>
              </div>
            ) : imports.map((item) => (
              <button
                key={item.id}
                type="button"
                className={`bridge-import-card${selectedId === item.id ? " is-active" : ""}`}
                onClick={() => setSelectedId(item.id)}
              >
                <span className="bridge-import-icon"><FileText size={16} /></span>
                <span className="bridge-import-copy">
                  <strong>{item.title || "Untitled import"}</strong>
                  <small>{item.source} · {item.artifacts.length} {item.artifacts.length === 1 ? "file" : "files"}</small>
                  <time dateTime={item.createdAt}>{new Date(item.createdAt).toLocaleDateString()}</time>
                </span>
              </button>
            ))}
          </nav>
        </aside>

        <section className="bridge-detail">
          {selectedImport ? (
            <>
              <div className="bridge-detail-scroll">
                <div className="bridge-file-hero">
                  <div className="bridge-file-symbol"><FileText size={22} /></div>
                  <div className="bridge-file-heading">
                    <span className="bridge-source">From {selectedImport.source}</span>
                    <h3>{selectedImport.title || "Untitled import"}</h3>
                    <p>{selectedImport.artifacts.length} {selectedImport.artifacts.length === 1 ? "file" : "files"} · {selectedImport.conversation.messages.length} context messages</p>
                  </div>
                  <div className="bridge-safety-note"><ShieldCheck size={15} /><span>Stored safely<br /><small>Nothing runs automatically</small></span></div>
                </div>

                <label className="bridge-workspace-field">
                  <span>Destination workspace</span>
                  <select value={workspaceId} onChange={(event) => setWorkspaceId(event.target.value)}>
                    <option value="">Choose where this should go…</option>
                    {workspaces.map((workspace) => <option key={workspace.id} value={workspace.id}>{workspace.name}</option>)}
                  </select>
                </label>

                <div className="bridge-section">
                  <div className="bridge-section-heading">
                    <div><span>Files</span><strong>{selectedImport.artifacts.length}</strong></div>
                    <small>Select a file to preview it</small>
                  </div>
                  {selectedImport.artifacts.length === 0 ? <p className="bridge-muted">This import contains conversation context only.</p> : (
                    <div className="bridge-artifacts">
                      {selectedImport.artifacts.map((artifact) => (
                        <button
                          type="button"
                          key={artifact.path}
                          className={artifactPreview?.path === artifact.path ? "is-active" : ""}
                          onClick={() => void handlePreviewArtifact(artifact.path)}
                        >
                          <span className="bridge-artifact-icon"><FileText size={16} /></span>
                          <span><strong>{artifact.path}</strong><small>{formatBytes(artifact.sizeBytes)} · verified {artifact.sha256.slice(0, 8)}</small></span>
                          <ArrowRight size={14} />
                        </button>
                      ))}
                    </div>
                  )}
                  {artifactPreview && (
                    <div className="bridge-preview">
                      <div className="bridge-preview-heading"><strong>{artifactPreview.path}</strong><span>Preview</span></div>
                      {previewImage ? <img src={previewImage} alt={artifactPreview.path} /> : previewText !== null ? <pre>{previewText}</pre> : <p>Preview is unavailable for this file type. Hopper preserved the original file without opening or executing it.</p>}
                    </div>
                  )}
                </div>

                <details className="bridge-context-panel">
                  <summary>
                    <span>Imported conversation</span>
                    <small>{selectedImport.conversation.messages.length} messages</small>
                  </summary>
                  <div className="bridge-messages">
                    {selectedImport.conversation.messages.map((message, index) => (
                      <article key={`${message.role}-${index}`} className={`bridge-message is-${message.role}`}>
                        <span>{message.role}</span><p>{message.content}</p>
                      </article>
                    ))}
                  </div>
                </details>
              </div>

              <footer className="bridge-handoff-bar">
                <label className="bridge-provider-field">
                  <span>Open with</span>
                  <select value={targetProviderId} onChange={(event) => setTargetProviderId(event.target.value)}>
                    {PROVIDERS.map((provider) => <option key={provider.id} value={provider.id}>{provider.label}</option>)}
                  </select>
                </label>
                <button className="bridge-copy-button" type="button" disabled={busy || !selectedWorkspace} onClick={() => void handleMaterialize()}><Download size={15} /> Copy only</button>
                <button className="bridge-launch-button" type="button" disabled={busy || !selectedWorkspace} onClick={() => void handleContinue()}>
                  {busy ? <LoaderCircle className="bridge-spin" size={16} /> : <ArrowRight size={16} />} Start in {targetProvider.label}
                </button>
              </footer>
            </>
          ) : <div className="bridge-empty bridge-detail-empty"><div className="bridge-empty-icon"><Archive size={20} /></div><strong>Select an import</strong><span>Its files and context will appear here for review.</span></div>}
        </section>
      </div>
    </div>
  );
}

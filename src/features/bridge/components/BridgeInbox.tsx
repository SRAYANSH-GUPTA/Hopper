import { useMemo, useState } from "react";
import { Archive, ArrowRight, Check, Code2, Download, Eye, FileText, FolderOpen, LoaderCircle, MessageSquare, Monitor, RefreshCw, Search, Smartphone, X } from "lucide-react";
import { PanelFrame, PanelHeader, PanelNavItem, PanelSearchField } from "@/features/design-system/components/panel/PanelPrimitives";
import { BRIDGE_PROVIDERS, useBridgeInbox, type BridgeInboxProps } from "@app/hooks/useBridgeInbox";
import { createHtmlPreview, decodeArtifactText } from "@/features/bridge/utils/artifactPreview";

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function sourceLabel(source: string): string {
  if (source === "file-import") return "File import";
  if (/chatgpt/i.test(source)) return "ChatGPT";
  if (/claude/i.test(source)) return "Claude";
  return source;
}

export function BridgeInbox(props: BridgeInboxProps) {
  const bridge = useBridgeInbox(props);
  const [search, setSearch] = useState("");
  const [view, setView] = useState<"preview" | "source" | "conversation">("preview");
  const [mobilePreview, setMobilePreview] = useState(false);
  const { selectedImport, preview, busy, workspace, provider } = bridge;
  const visibleImports = bridge.imports.filter((item) =>
    `${item.title ?? ""} ${item.source} ${item.artifacts.map((artifact) => artifact.path).join(" ")}`.toLowerCase().includes(search.toLowerCase()),
  );
  const text = useMemo(() => preview ? decodeArtifactText(preview) : null, [preview]);
  const html = useMemo(() =>
    preview && text !== null && (preview.mimeType === "text/html" || /\.html?$/i.test(preview.path))
      ? createHtmlPreview(text) : null,
  [preview, text]);
  const image = preview?.mimeType?.startsWith("image/") ? `data:${preview.mimeType};base64,${preview.contentBase64}` : null;
  const canContinue = Boolean(selectedImport && workspace && !busy && !bridge.detailLoading);
  const selectedArtifact = selectedImport?.artifacts.find((artifact) => artifact.path === bridge.artifactPath);

  return (
    <div className="bridge-inbox" aria-label="Bridge inbox">
      <header className="bridge-header">
        <div className="bridge-title"><Archive size={22} /><div><h2>Bridge</h2><p>From a conversation to your next creation.</p></div></div>
        <div className="bridge-header-actions">
          <button className="ghost icon-button" type="button" disabled={bridge.loading || Boolean(busy)} onClick={() => void bridge.refresh()} aria-label="Refresh inbox"><RefreshCw size={16} /></button>
          <button className="secondary bridge-button" type="button" disabled={Boolean(busy)} onClick={() => void bridge.runAction("import")}>
            {busy === "import" ? <LoaderCircle className="bridge-spin" size={16} /> : <Download size={16} />} Import file
          </button>
        </div>
      </header>

      {bridge.error && (
        <div className="bridge-feedback is-error" role="alert">
          <div><strong>Couldn’t complete that action</strong><p>{bridge.error}</p></div>
          <button className="ghost icon-button" type="button" onClick={() => bridge.setError(null)} aria-label="Dismiss error"><X size={16} /></button>
        </div>
      )}
      {bridge.notice && <div className="bridge-feedback" role="status"><Check size={16} /><p>{bridge.notice}</p></div>}

      <div className="bridge-layout">
        <PanelFrame className="bridge-library">
          <PanelHeader><h3>Inbox <span>{bridge.imports.length}</span></h3></PanelHeader>
          <PanelSearchField icon={<Search />} placeholder="Find an import…" aria-label="Find an import" value={search} onChange={(event) => setSearch(event.target.value)} />
          <nav className="bridge-import-list" aria-label="Imported files">
            {bridge.loading && bridge.imports.length === 0 ? <p className="bridge-quiet" role="status">Loading imports…</p> : visibleImports.map((item) => (
              <PanelNavItem key={item.id} active={bridge.selectedId === item.id} disabled={Boolean(busy)} aria-current={bridge.selectedId === item.id ? "true" : undefined}
                className="bridge-import-item" onClick={() => { bridge.setSelectedId(item.id); setView("preview"); }}>
                <div className="bridge-import-heading"><span className="bridge-file-type">{item.artifacts[0]?.path.split(".").pop()?.slice(0, 5).toUpperCase() || "CHAT"}</span><time dateTime={item.createdAt}>{new Date(item.createdAt).toLocaleDateString(undefined, { month: "short", day: "numeric" })}</time></div>
                <strong title={item.title ?? ""}>{item.title || "Untitled import"}</strong>
                <small>{sourceLabel(item.source)}<span>{item.artifacts.length} {item.artifacts.length === 1 ? "file" : "files"}</span></small>
              </PanelNavItem>
            ))}
            {!bridge.loading && visibleImports.length === 0 && <div className="bridge-list-empty"><Archive size={24} /><strong>{search ? "No matches" : "Your inbox is ready"}</strong><p>{search ? "Try another filename." : "Download a file inside an assistant or import one from your computer."}</p></div>}
          </nav>
          <p className="bridge-library-note">Downloads from Hopper’s AI browser arrive here.</p>
        </PanelFrame>

        <section className="bridge-review" aria-label="Review import">
          {bridge.detailLoading ? <div className="bridge-empty" role="status"><LoaderCircle className="bridge-spin" size={24} /><p>Opening import…</p></div> : selectedImport ? (
            <>
              <header className="bridge-review-heading">
                <div><p>{sourceLabel(selectedImport.source)}</p><h3 title={selectedImport.title ?? ""}>{selectedImport.title || "Untitled import"}</h3></div>
                <span className="bridge-review-count">{selectedImport.artifacts.length} {selectedImport.artifacts.length === 1 ? "file" : "files"}</span>
              </header>
              <div className="bridge-review-body">
                <div className="bridge-canvas">
                  <div className="bridge-tabs" role="group" aria-label="Review view">
                    <button type="button" aria-pressed={view === "preview"} onClick={() => setView("preview")}><Eye size={15} />Preview</button>
                    <button type="button" aria-pressed={view === "source"} onClick={() => setView("source")}><Code2 size={15} />Source</button>
                    <button type="button" aria-pressed={view === "conversation"} onClick={() => setView("conversation")}><MessageSquare size={15} />Context</button>
                  </div>
                  {view === "conversation" ? (
                    <div className="bridge-conversation">
                      <h4>Imported context</h4>
                      {selectedImport.conversation.messages.length === 0 && <p className="bridge-quiet">No conversation was included with these files.</p>}
                      {selectedImport.conversation.messages.map((message, index) => <article key={index}><strong>{message.role}</strong><p>{message.content}</p></article>)}
                    </div>
                  ) : (
                    <>
                      <div className="bridge-preview-toolbar">
                        <FileText size={14} /><span title={bridge.artifactPath}>{bridge.artifactPath || "No file selected"}</span>
                        {selectedArtifact && <small>{formatBytes(selectedArtifact.sizeBytes)}</small>}
                        {html && view === "preview" && <div className="bridge-preview-size" role="group" aria-label="Preview width">
                          <button type="button" aria-label="Desktop preview" aria-pressed={!mobilePreview} onClick={() => setMobilePreview(false)}><Monitor size={15} /></button>
                          <button type="button" aria-label="Mobile preview" aria-pressed={mobilePreview} onClick={() => setMobilePreview(true)}><Smartphone size={15} /></button>
                        </div>}
                      </div>
                      <div className={`bridge-preview-stage${mobilePreview && html && view === "preview" ? " is-mobile" : ""}`}>
                        {bridge.previewLoading ? <div className="bridge-empty" role="status"><LoaderCircle className="bridge-spin" size={22} /><p>Loading preview…</p></div>
                          : bridge.previewError ? <div className="bridge-empty" role="alert"><FileText size={28} /><strong>Couldn’t load this file</strong><p>{bridge.previewError}</p></div>
                          : !preview ? <div className="bridge-empty"><MessageSquare size={28} /><strong>Conversation only</strong><p>Open Context to review the imported messages.</p></div>
                          : view === "preview" && html ? <iframe title={`Preview of ${preview.path}`} sandbox="" referrerPolicy="no-referrer" srcDoc={html} />
                          : view === "preview" && image ? <div className="bridge-image-preview"><img src={image} alt={preview.path} /></div>
                          : text !== null ? <pre className="bridge-source-code"><code>{text}</code></pre>
                          : <div className="bridge-empty"><FileText size={32} /><strong>{preview.path}</strong><p>This file is ready to copy. An inline preview isn’t available for this format.</p></div>}
                      </div>
                      {html && view === "preview" && <p className="bridge-preview-note">Static preview. Scripts and external assets are disabled.</p>}
                    </>
                  )}
                </div>

                <div className="bridge-transfer">
                  <section className="bridge-bundle-files">
                    <h4>Included files <span>{selectedImport.artifacts.length}</span></h4>
                    <div className="bridge-file-list">
                      {selectedImport.artifacts.map((artifact) => (
                        <button type="button" key={artifact.path} aria-pressed={bridge.artifactPath === artifact.path} title={artifact.path}
                          onClick={() => { bridge.setArtifactPath(artifact.path); setView("preview"); }}>
                          <FileText size={17} /><span><strong>{artifact.path}</strong><small>{formatBytes(artifact.sizeBytes)}</small></span>
                          {bridge.artifactPath === artifact.path && <Check size={14} />}
                        </button>
                      ))}
                      {selectedImport.artifacts.length === 0 && <p className="bridge-quiet">No files attached.</p>}
                    </div>
                  </section>
                  <section className="bridge-destination">
                    <h4><FolderOpen size={17} />Continue your work</h4>
                    <p>Bring the files and context into a project.</p>
                    <label>Workspace
                      <select value={bridge.workspaceId} disabled={Boolean(busy)} onChange={(event) => bridge.setWorkspaceId(event.target.value)}>
                        <option value="">Choose a workspace…</option>
                        {props.workspaces.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}
                      </select>
                    </label>
                    {workspace && <p className="bridge-workspace-path" title={workspace.path}>{workspace.path}</p>}
                    <label>Agent
                      <select value={bridge.providerId} disabled={Boolean(busy)} onChange={(event) => bridge.setProviderId(event.target.value)}>
                        {BRIDGE_PROVIDERS.map((item) => <option key={item.id} value={item.id}>{item.label}</option>)}
                      </select>
                    </label>
                    <button className="primary bridge-button bridge-start" type="button" disabled={!canContinue} onClick={() => void bridge.runAction("start")}>
                      {busy === "start" ? <LoaderCircle className="bridge-spin" size={16} /> : <ArrowRight size={16} />}
                      {busy === "start" ? "Preparing…" : `Continue in ${provider.label}`}
                    </button>
                    <button className="ghost bridge-button" type="button" disabled={!canContinue} onClick={() => void bridge.runAction("copy")}>
                      {busy === "copy" ? <LoaderCircle className="bridge-spin" size={16} /> : <Download size={16} />}
                      {busy === "copy" ? "Copying…" : "Copy files only"}
                    </button>
                    <p className="bridge-transfer-note">{!workspace ? "Choose a workspace to enable these actions." : "Copies files into .hopper/imports. Context is attached to your first agent message."}</p>
                  </section>
                </div>
              </div>
            </>
          ) : <div className="bridge-empty bridge-welcome"><Archive size={38} /><h3>Bring your ideas here.</h3><p>Import a design, document, or conversation.<br />Review it, choose a project, and keep building.</p><button className="primary bridge-button" type="button" disabled={Boolean(busy)} onClick={() => void bridge.runAction("import")}><Download size={16} />Import your first file</button></div>}
        </section>
      </div>
    </div>
  );
}

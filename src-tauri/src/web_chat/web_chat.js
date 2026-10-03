// Injected by Hopper into embedded assistant tabs. Answers Hopper's requests to
// read the open conversation or paste text into the chat's input box.
//
// Hopper starts every exchange by calling `window.__hopperWebChat.<method>` with
// a request id; the page answers once through `web_chat_reply`.
(() => {
  if (window.top !== window) return;
  const SITES = {
    "chatgpt.com": "chatgpt",
    "chat.openai.com": "chatgpt",
    "claude.ai": "claude",
    "gemini.google.com": "gemini",
    "copilot.microsoft.com": "copilot",
    "chat.mistral.ai": "mistral",
  };
  const host = location.hostname;
  const siteHost = Object.keys(SITES).find((allowed) => host === allowed || host.endsWith(`.${allowed}`));
  if (location.protocol !== "https:" || !siteHost) return;
  const site = SITES[siteHost];
  const internals = window.__TAURI_INTERNALS__;
  if (!internals || typeof internals.invoke !== "function") return;
  const invoke = internals.invoke.bind(internals);

  const NO_CONVERSATION = "Open a conversation in this tab first.";

  // ── Capture ────────────────────────────────────────────────────────────────

  async function fetchJson(url, init = {}) {
    const response = await fetch(url, { credentials: "include", ...init });
    if (!response.ok) throw new Error(`Request failed (${response.status}).`);
    return response.json();
  }

  function chatGptText(message) {
    const content = message?.content;
    if (!content || !["text", "multimodal_text"].includes(content.content_type)) return "";
    return (content.parts ?? []).filter((part) => typeof part === "string").join("\n").trim();
  }

  async function captureChatGpt() {
    const id = location.pathname.match(/\/c\/([0-9a-f-]{8,})/i)?.[1];
    if (!id) throw new Error(NO_CONVERSATION);
    try {
      const session = await fetchJson("/api/auth/session");
      const headers = session?.accessToken ? { Authorization: `Bearer ${session.accessToken}` } : {};
      const data = await fetchJson(`/backend-api/conversation/${id}`, { headers });
      const mapping = data?.mapping ?? {};
      const messages = [];
      const seen = new Set();
      // Follow the visible branch from the newest node back to the root.
      for (let nodeId = data?.current_node; nodeId && mapping[nodeId] && !seen.has(nodeId); nodeId = mapping[nodeId].parent) {
        seen.add(nodeId);
        const message = mapping[nodeId].message;
        const role = message?.author?.role;
        if (role !== "user" && role !== "assistant") continue;
        if (message.recipient && message.recipient !== "all") continue;
        if (message.metadata?.is_visually_hidden_from_conversation) continue;
        messages.push({ role, content: chatGptText(message) });
      }
      messages.reverse();
      if (messages.some((message) => message.content)) return { title: data.title, messages };
    } catch {
      // Fall back to the rendered conversation.
    }
    return captureDom();
  }

  function claudeText(message) {
    if (Array.isArray(message?.content)) {
      const text = message.content
        .filter((block) => block?.type === "text" && typeof block.text === "string")
        .map((block) => block.text)
        .join("\n\n");
      if (text.trim()) return text;
    }
    return typeof message?.text === "string" ? message.text : "";
  }

  function claudeMessages(data) {
    const all = Array.isArray(data?.chat_messages) ? data.chat_messages : [];
    const byId = new Map(all.map((message) => [message.uuid, message]));
    let chain = all;
    if (byId.has(data?.current_leaf_message_uuid)) {
      chain = [];
      const seen = new Set();
      // Follow the visible branch from the newest message back to the root.
      for (let message = byId.get(data.current_leaf_message_uuid); message && !seen.has(message.uuid); message = byId.get(message.parent_message_uuid)) {
        seen.add(message.uuid);
        chain.push(message);
      }
      chain.reverse();
    }
    return chain.map((message) => ({
      role: message.sender === "human" ? "user" : message.sender,
      content: claudeText(message),
    }));
  }

  async function captureClaude() {
    const id = location.pathname.match(/\/chat\/([0-9a-f-]{36})/i)?.[1];
    if (!id) throw new Error(NO_CONVERSATION);
    try {
      const organizations = await fetchJson("/api/organizations");
      const ids = (Array.isArray(organizations) ? organizations : []).map((org) => org?.uuid).filter(Boolean);
      const active = document.cookie.match(/(?:^|;\s*)lastActiveOrg=([^;]+)/)?.[1];
      const ordered = active ? [active, ...ids.filter((org) => org !== active)] : ids;
      for (const org of ordered) {
        try {
          const data = await fetchJson(
            `/api/organizations/${org}/chat_conversations/${id}?tree=True&rendering_mode=messages&render_all_tools=true`,
          );
          const messages = claudeMessages(data);
          if (messages.some((message) => message.content)) return { title: data.name, messages };
        } catch {
          // Try the next organization.
        }
      }
    } catch {
      // Fall back to the rendered conversation.
    }
    return captureDom();
  }

  // Rendered-page turns per site, in document order.
  const DOM_TURNS = {
    chatgpt: {
      selector: "[data-message-author-role]",
      role: (el) => el.getAttribute("data-message-author-role"),
      content: (el) => el.querySelector(".markdown") ?? el,
    },
    claude: {
      selector: '[data-testid="user-message"], .font-claude-response',
      role: (el) => (el.matches('[data-testid="user-message"]') ? "user" : "assistant"),
    },
    gemini: {
      selector: "user-query, model-response",
      role: (el) => (el.tagName.toLowerCase() === "user-query" ? "user" : "assistant"),
      content: (el) => el.querySelector(".query-text, message-content") ?? el,
    },
    copilot: {
      selector: '[data-content="user-message"], [data-content="ai-message"]',
      role: (el) => (el.getAttribute("data-content") === "user-message" ? "user" : "assistant"),
    },
  };
  const GENERIC_TURNS = {
    selector: '[data-message-author-role], [data-role="user"], [data-role="assistant"]',
    role: (el) => el.getAttribute("data-message-author-role") ?? el.getAttribute("data-role"),
  };

  function captureDom() {
    const turns = DOM_TURNS[site] ?? GENERIC_TURNS;
    const matches = Array.from(document.querySelectorAll(turns.selector));
    const outermost = matches.filter((el) => !matches.some((other) => other !== el && other.contains(el)));
    const messages = outermost.map((el) => ({
      role: turns.role(el),
      content: domToMarkdown(turns.content?.(el) ?? el),
    }));
    if (!messages.some((message) => message.content)) {
      throw new Error(`${NO_CONVERSATION} If it is a long chat, scroll up so its older messages load.`);
    }
    return { title: pageTitle(), messages };
  }

  function pageTitle() {
    const title = document.title
      .replace(/\s*[-|–—]\s*(ChatGPT|Claude|Gemini|Microsoft Copilot|Copilot|Le Chat|Mistral)[^-|–—]*$/i, "")
      .trim();
    return /^(ChatGPT|Claude|Gemini|Microsoft Copilot|Copilot|Le Chat|Mistral AI|Mistral)$/i.test(title) ? null : title;
  }

  // Turns rendered message DOM back into Markdown the agent can read.
  function domToMarkdown(root) {
    const codeBlocks = [];
    const SKIP = new Set(["SCRIPT", "STYLE", "NOSCRIPT", "BUTTON", "SVG", "TEMPLATE"]);
    const BLOCK = new Set(["P", "DIV", "SECTION", "ARTICLE", "HEADER", "FOOTER", "FIGURE", "DETAILS", "SUMMARY"]);

    function children(el, depth) {
      return Array.from(el.childNodes).map((child) => walk(child, depth)).join("");
    }

    function walk(node, depth) {
      if (node.nodeType === Node.TEXT_NODE) {
        const whiteSpace = node.parentElement ? getComputedStyle(node.parentElement).whiteSpace : "normal";
        return whiteSpace.startsWith("pre") || whiteSpace === "break-spaces"
          ? node.nodeValue
          : node.nodeValue.replace(/\s+/g, " ");
      }
      if (node.nodeType !== Node.ELEMENT_NODE) return "";
      const el = node;
      const tag = el.tagName.toUpperCase();
      if (SKIP.has(tag) || el.getAttribute("aria-hidden") === "true") return "";
      if (tag === "PRE") {
        const code = el.querySelector("code") ?? el;
        const language = (Array.from(code.classList).find((name) => name.startsWith("language-")) ?? "").slice(9);
        codeBlocks.push(`\`\`\`${language}\n${(code.textContent ?? "").replace(/\n$/, "")}\n\`\`\``);
        return `\n\n\u0000${codeBlocks.length - 1}\u0000\n\n`;
      }
      if (tag === "CODE") return `\`${el.textContent ?? ""}\``;
      if (tag === "BR") return "\n";
      if (tag === "HR") return "\n\n---\n\n";
      if (/^H[1-6]$/.test(tag)) return `\n\n${"#".repeat(Number(tag[1]))} ${children(el, depth).trim()}\n\n`;
      if (tag === "STRONG" || tag === "B") return `**${children(el, depth)}**`;
      if (tag === "EM" || tag === "I") return `*${children(el, depth)}*`;
      if (tag === "A") {
        const text = children(el, depth);
        const href = el.getAttribute("href") ?? "";
        return /^https?:/i.test(href) && text.trim() && text.trim() !== href ? `[${text.trim()}](${href})` : text;
      }
      if (tag === "UL" || tag === "OL") {
        const items = Array.from(el.children).filter((child) => child.tagName.toUpperCase() === "LI");
        const lines = items.map((item, index) => {
          const marker = tag === "OL" ? `${index + 1}.` : "-";
          return `${"  ".repeat(depth)}${marker} ${children(item, depth + 1).trim()}`;
        });
        return `\n\n${lines.join("\n")}\n\n`;
      }
      if (tag === "BLOCKQUOTE") {
        return `\n\n${children(el, depth).trim().split("\n").map((line) => `> ${line}`).join("\n")}\n\n`;
      }
      if (tag === "TABLE") {
        const rows = Array.from(el.querySelectorAll("tr")).map((row) =>
          `| ${Array.from(row.children).map((cell) => children(cell, depth).trim().replace(/\|/g, "\\|")).join(" | ")} |`);
        if (rows.length > 1) {
          const columns = el.querySelector("tr")?.children.length ?? 1;
          rows.splice(1, 0, `|${" --- |".repeat(columns)}`);
        }
        return `\n\n${rows.join("\n")}\n\n`;
      }
      const content = children(el, depth);
      return BLOCK.has(tag) ? `\n\n${content}\n\n` : content;
    }

    return walk(root, 0)
      .replace(/[ \t]+\n/g, "\n")
      .replace(/\n{3,}/g, "\n\n")
      .replace(/\u0000(\d+)\u0000/g, (_, index) => codeBlocks[Number(index)])
      .trim();
  }

  // Keeps user/assistant turns and joins consecutive parts of the same turn.
  function finalize(result) {
    const messages = [];
    for (const message of result.messages) {
      const content = (message.content ?? "").trim();
      if (!content || (message.role !== "user" && message.role !== "assistant")) continue;
      const previous = messages[messages.length - 1];
      if (previous?.role === message.role) previous.content += `\n\n${content}`;
      else messages.push({ role: message.role, content });
    }
    if (!messages.length) throw new Error(NO_CONVERSATION);
    return { title: typeof result.title === "string" ? result.title : null, messages };
  }

  async function capture() {
    if (site === "chatgpt") return finalize(await captureChatGpt());
    if (site === "claude") return finalize(await captureClaude());
    return finalize(captureDom());
  }

  // ── Insert ─────────────────────────────────────────────────────────────────

  const COMPOSERS = {
    chatgpt: ["#prompt-textarea"],
    claude: ['div.ProseMirror[contenteditable="true"]'],
    gemini: ["rich-textarea .ql-editor", 'div.ql-editor[contenteditable="true"]'],
    copilot: ["textarea#userInput"],
    mistral: ['div.ProseMirror[contenteditable="true"]'],
  };

  function isVisible(el) {
    const rect = el.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  }

  function findComposer() {
    for (const selector of [...(COMPOSERS[site] ?? []), "textarea", '[contenteditable="true"]']) {
      const el = Array.from(document.querySelectorAll(selector)).find(isVisible);
      if (el) return el;
    }
    return null;
  }

  function insertIntoField(el, text) {
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    const setValue = Object.getOwnPropertyDescriptor(proto, "value")?.set;
    const start = el.selectionStart ?? el.value.length;
    const end = el.selectionEnd ?? el.value.length;
    const next = el.value.slice(0, start) + text + el.value.slice(end);
    if (setValue) setValue.call(el, next);
    else el.value = next;
    el.dispatchEvent(new Event("input", { bubbles: true }));
  }

  function insertIntoEditor(el, text) {
    const selection = window.getSelection();
    if (selection && !el.contains(selection.anchorNode)) {
      const range = document.createRange();
      range.selectNodeContents(el);
      range.collapse(false);
      selection.removeAllRanges();
      selection.addRange(range);
    }
    // Rich editors (ProseMirror, Quill) handle paste natively, including long text.
    try {
      const data = new DataTransfer();
      data.setData("text/plain", text);
      const event = new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true });
      if (!el.dispatchEvent(event)) return;
    } catch {
      // Fall through to a plain text insert.
    }
    const before = el.textContent ?? "";
    if (!document.execCommand("insertText", false, text) && (el.textContent ?? "") === before) {
      throw new Error("Hopper couldn't paste into this chat's input box.");
    }
  }

  function insert(text) {
    const el = findComposer();
    if (!el) throw new Error("Couldn't find this chat's input box.");
    el.focus();
    if (el instanceof HTMLTextAreaElement || el instanceof HTMLInputElement) insertIntoField(el, text);
    else insertIntoEditor(el, text);
    return null;
  }

  // ── File transfer ──────────────────────────────────────────────────────────
  // Hovering a downloadable file link shows a floating "Send to Hopper" button.
  // The button lives outside the page's own DOM tree so the assistant's UI
  // framework never sees a foreign node.

  const FILE_LINK = /\.(?:pdf|docx?|xlsx?|csv|tsv|pptx?|zip|tar|gz|json|txt|md|rtf|html?|py|js|ts|png|jpe?g|gif|webp|svg|mp3|wav|mp4|mov)(?:$|[?#])/i;
  const FETCH_FAILED = "Hopper couldn't fetch this file. Download it instead, and Hopper will offer to send the download.";
  const LABEL_IDLE = "Send to Hopper";

  function linkHref(link) {
    return link.getAttribute("href") ?? "";
  }

  function fileNameFromLink(link) {
    const download = link.getAttribute("download");
    if (download) return download;
    const href = linkHref(link);
    try {
      const path = href.startsWith("sandbox:") ? href.slice("sandbox:".length) : new URL(link.href, location.href).pathname;
      const name = decodeURIComponent(path.split("/").pop() ?? "");
      if (name) return name;
    } catch {
      // Fall back to the visible link label.
    }
    return (link.textContent ?? "web-chat-file").trim().slice(0, 160) || "web-chat-file";
  }

  function isDownloadableFile(link) {
    const href = linkHref(link);
    return Boolean(
      link.hasAttribute("download")
      || href.startsWith("sandbox:")
      || href.startsWith("blob:")
      || FILE_LINK.test(href)
      || /\/(?:files|attachments|download)\b/i.test(href),
    );
  }

  // ChatGPT's generated files are `sandbox:` links that resolve to a signed URL.
  async function chatGptSandboxUrl(link) {
    const conversationId = location.pathname.match(/\/c\/([0-9a-f-]{8,})/i)?.[1];
    const messageId = link.closest("[data-message-id]")?.getAttribute("data-message-id");
    if (!conversationId || !messageId) throw new Error(FETCH_FAILED);
    const session = await fetchJson("/api/auth/session");
    const headers = session?.accessToken ? { Authorization: `Bearer ${session.accessToken}` } : {};
    const sandboxPath = linkHref(link).slice("sandbox:".length);
    const data = await fetchJson(
      `/backend-api/conversation/${conversationId}/interpreter/download?message_id=${encodeURIComponent(messageId)}&sandbox_path=${encodeURIComponent(sandboxPath)}`,
      { headers },
    );
    if (typeof data?.download_url !== "string") throw new Error(FETCH_FAILED);
    return data.download_url;
  }

  async function fetchFileBlob(link) {
    const url = linkHref(link).startsWith("sandbox:")
      ? (site === "chatgpt" ? await chatGptSandboxUrl(link) : null)
      : link.href;
    if (!url) throw new Error(FETCH_FAILED);
    // Signed file URLs often reject credentialed cross-origin requests; retry without.
    for (const credentials of ["include", "omit"]) {
      try {
        const response = await fetch(url, { credentials });
        if (response.ok) return await response.blob();
      } catch {
        // Try the next mode.
      }
    }
    throw new Error(FETCH_FAILED);
  }

  function blobToBase64(blob) {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onerror = () => reject(new Error("Couldn't read this file."));
      reader.onload = () => {
        const dataUrl = String(reader.result ?? "");
        resolve(dataUrl.slice(dataUrl.indexOf(",") + 1));
      };
      reader.readAsDataURL(blob);
    });
  }

  let fileButton = null;
  let activeLink = null;
  let hideTimer = 0;
  let resetTimer = 0;
  let transferring = false;

  function setButtonState(label, title) {
    if (!fileButton) return;
    fileButton.textContent = label;
    fileButton.title = title;
    fileButton.setAttribute("aria-label", title);
  }

  function positionFileButton() {
    if (!fileButton || !activeLink) return;
    const rect = activeLink.getBoundingClientRect();
    if (!activeLink.isConnected || rect.bottom < 0 || rect.top > window.innerHeight) {
      fileButton.style.display = "none";
      return;
    }
    fileButton.style.display = "inline-flex";
    const top = Math.max(4, rect.top + rect.height / 2 - fileButton.offsetHeight / 2);
    const left = Math.min(window.innerWidth - fileButton.offsetWidth - 4, rect.right + 6);
    fileButton.style.top = `${top}px`;
    fileButton.style.left = `${Math.max(4, left)}px`;
  }

  function scheduleHide() {
    window.clearTimeout(hideTimer);
    hideTimer = window.setTimeout(() => {
      if (transferring) return;
      activeLink = null;
      if (fileButton) fileButton.style.display = "none";
    }, 500);
  }

  async function transferActiveFile() {
    const link = activeLink;
    if (!link || transferring) return;
    transferring = true;
    window.clearTimeout(resetTimer);
    const fileName = fileNameFromLink(link);
    setButtonState("Adding…", `Adding ${fileName} to Hopper`);
    fileButton.disabled = true;
    try {
      const blob = await fetchFileBlob(link);
      if (!blob.size) throw new Error("This file is empty.");
      await invoke("web_chat_file_import", {
        payload: {
          fileName,
          mimeType: blob.type || null,
          contentBase64: await blobToBase64(blob),
        },
      });
      setButtonState("Added to Hopper ✓", `${fileName} is attached to your Hopper chat`);
    } catch (error) {
      setButtonState("Couldn't add file", error instanceof Error ? error.message : String(error));
    } finally {
      transferring = false;
      fileButton.disabled = false;
      resetTimer = window.setTimeout(() => {
        if (activeLink) setButtonState(LABEL_IDLE, `Attach ${fileNameFromLink(activeLink)} to your Hopper chat`);
        scheduleHide();
      }, 2500);
    }
  }

  function createFileButton() {
    const button = document.createElement("button");
    button.type = "button";
    button.style.cssText = [
      "all:initial", "position:fixed", "z-index:2147483647", "display:none", "box-sizing:border-box",
      "align-items:center", "min-height:26px", "padding:4px 9px", "border-radius:6px",
      "border:1px solid rgba(255,255,255,.2)", "background:rgba(32,33,36,.92)", "color:#fff",
      "font:600 11px/1.2 system-ui,sans-serif", "cursor:pointer", "white-space:nowrap",
      "box-shadow:0 2px 8px rgba(0,0,0,.25)",
    ].join(";");
    button.addEventListener("mouseenter", () => window.clearTimeout(hideTimer));
    button.addEventListener("mouseleave", scheduleHide);
    button.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      void transferActiveFile();
    });
    document.documentElement.appendChild(button);
    return button;
  }

  document.addEventListener("mouseover", (event) => {
    const target = event.target instanceof Element ? event.target : null;
    if (!target || target === fileButton) return;
    const link = target.closest("a[href]");
    if (!link || !isDownloadableFile(link)) {
      if (activeLink) scheduleHide();
      return;
    }
    window.clearTimeout(hideTimer);
    if (transferring) return;
    fileButton ??= createFileButton();
    if (link !== activeLink) {
      activeLink = link;
      window.clearTimeout(resetTimer);
      setButtonState(LABEL_IDLE, `Attach ${fileNameFromLink(link)} to your Hopper chat`);
    }
    positionFileButton();
  }, true);
  window.addEventListener("scroll", positionFileButton, true);
  window.addEventListener("resize", positionFileButton);

  // ── Requests ───────────────────────────────────────────────────────────────

  async function run(requestId, task) {
    let payload;
    try {
      payload = { requestId, ok: true, data: await task() };
    } catch (error) {
      payload = { requestId, ok: false, error: error instanceof Error ? error.message : String(error) };
    }
    await invoke("web_chat_reply", { payload }).catch(() => {});
  }

  Object.defineProperty(window, "__hopperWebChat", {
    value: Object.freeze({
      capture: (requestId) => void run(requestId, capture),
      insert: (requestId, text) => void run(requestId, () => insert(String(text))),
    }),
  });
})();

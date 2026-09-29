import type { BridgeArtifactContent } from "@services/tauri";

export function decodeArtifactText(content: BridgeArtifactContent): string | null {
  if (!content.mimeType?.startsWith("text/") && !/\.(md|txt|json|html?|css|js|jsx|ts|tsx|svg)$/i.test(content.path)) {
    return null;
  }
  const bytes = Uint8Array.from(window.atob(content.contentBase64), (character) => character.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

export function createHtmlPreview(html: string): string {
  const document = new DOMParser().parseFromString(html, "text/html");
  document.querySelectorAll("script, iframe, frame, frameset, object, embed, base, meta, link").forEach((element) => element.remove());
  document.querySelectorAll("*").forEach((element) => {
    for (const attribute of Array.from(element.attributes)) {
      if (/^on/i.test(attribute.name) || ["href", "xlink:href", "action", "formaction", "target", "srcdoc"].includes(attribute.name)) {
        element.removeAttribute(attribute.name);
      }
    }
  });
  const policy = document.createElement("meta");
  policy.httpEquiv = "Content-Security-Policy";
  policy.content = "default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data:; base-uri 'none'; form-action 'none'";
  document.head.prepend(policy);
  const viewport = document.createElement("meta");
  viewport.name = "viewport";
  viewport.content = "width=device-width, initial-scale=1";
  document.head.append(viewport);
  return "<!doctype html>" + document.documentElement.outerHTML;
}

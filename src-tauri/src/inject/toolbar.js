// Optional compact browser controls, off by default (show_toolbar). Rendered as
// a small floating cluster in the bottom-left corner rather than a full-width
// top bar: a top bar has to reserve vertical space, and no injected CSS can
// reserve it on sites with a `position: fixed` header (e.g. YouTube's masthead)
// without either clipping that header or breaking the site's own scrolling.
// A floating cluster reserves no layout space, so it can never clip page chrome.
document.addEventListener("DOMContentLoaded", () => {
  if (window.pakeConfig?.show_toolbar !== true) return;
  if (document.getElementById("pake-toolbar")) return;

  const invoke = window.__TAURI__?.core?.invoke;
  const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  const c = dark
    ? { bg: "rgba(35,35,35,.92)", fg: "#e8e8e8", bd: "rgba(255,255,255,.12)", hv: "rgba(255,255,255,.14)" }
    : { bg: "rgba(250,250,250,.92)", fg: "#222", bd: "rgba(0,0,0,.12)", hv: "rgba(0,0,0,.08)" };

  const bar = document.createElement("div");
  bar.id = "pake-toolbar";
  bar.style.cssText = `
    position: fixed; bottom: 16px; left: 16px; z-index: 2147483647;
    display: flex; align-items: center; gap: 1px; padding: 3px;
    border-radius: 10px; background: ${c.bg}; color: ${c.fg};
    border: 1px solid ${c.bd}; box-shadow: 0 2px 12px rgba(0,0,0,.25);
    backdrop-filter: blur(8px);
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    opacity: .55; transition: opacity .15s; user-select: none;
  `;
  bar.addEventListener("mouseenter", () => (bar.style.opacity = "1"));
  bar.addEventListener("mouseleave", () => (bar.style.opacity = ".55"));

  function addButton(label, title, onClick) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.textContent = label;
    btn.title = title;
    btn.style.cssText = `
      border: none; background: transparent; color: inherit; cursor: pointer;
      width: 30px; height: 28px; border-radius: 7px; font-size: 15px;
      line-height: 1; padding: 0;
    `;
    btn.addEventListener("mouseenter", () => (btn.style.background = c.hv));
    btn.addEventListener("mouseleave", () => (btn.style.background = "transparent"));
    btn.addEventListener("click", onClick);
    bar.appendChild(btn);
    return btn;
  }

  function addSeparator() {
    const sep = document.createElement("span");
    sep.style.cssText = `width: 1px; height: 16px; background: ${c.bd}; margin: 0 3px;`;
    bar.appendChild(sep);
  }

  addButton("←", "Back", () => window.history.back());
  addButton("→", "Forward", () => window.history.forward());
  addButton("⟳", "Reload", () => window.location.reload());
  addButton("⌂", "Home", () => window.pakeGoHome && window.pakeGoHome());
  addButton("⊞", "Open current page in a new window", () =>
    window.pakeOpenInNewWindow && window.pakeOpenInNewWindow(window.location.href),
  );
  addSeparator();
  addButton("⧉", "Copy URL", () => {
    navigator.clipboard.writeText(window.location.href);
    if (window.pakeToast) window.pakeToast("URL copied");
  });
  if (invoke) {
    addButton("↗", "Open in default browser", () => {
      invoke("plugin:shell|open", { path: window.location.href }).catch(
        (error) => console.error("[Pake] Failed to open browser:", error),
      );
    });
  }
  if (window.pakeConfig?.translation_target) {
    addButton(
      "文A",
      `Translate to ${window.pakeConfig.translation_target}`,
      () => window.pakeTranslate && window.pakeTranslate(),
    );
  }
  const adblockMode = window.pakeConfig?.adblock_mode;
  if (adblockMode === "basic" || adblockMode === "strict") {
    addSeparator();
    const badge = document.createElement("span");
    badge.textContent = "🛡";
    badge.title = `Ad blocking: ${adblockMode}`;
    badge.style.cssText = "padding: 0 5px; font-size: 14px;";
    bar.appendChild(badge);
  }

  document.body.appendChild(bar);
});

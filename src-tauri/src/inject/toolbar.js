// Optional compact navigation toolbar, off by default (show_toolbar). Pure
// DOM/CSS, no dependencies; sized to stay out of the way on app-like sites.
document.addEventListener("DOMContentLoaded", () => {
  if (window.pakeConfig?.show_toolbar !== true) return;
  if (document.getElementById("pake-toolbar")) return;

  const invoke = window.__TAURI__?.core?.invoke;
  const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  const colors = dark
    ? { bg: "#2d2d2d", fg: "#e0e0e0", border: "#404040", hover: "#404040" }
    : { bg: "#f6f6f6", fg: "#333333", border: "#e0e0e0", hover: "#e4e4e4" };
  const BAR_HEIGHT = 34;

  const bar = document.createElement("div");
  bar.id = "pake-toolbar";
  bar.style.cssText = `
    position: fixed; top: 0; left: 0; right: 0; height: ${BAR_HEIGHT}px;
    display: flex; align-items: center; gap: 2px; padding: 0 6px;
    background: ${colors.bg}; color: ${colors.fg};
    border-bottom: 1px solid ${colors.border};
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    font-size: 14px; z-index: 2147483646; user-select: none;
  `;

  function addButton(label, title, onClick) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.textContent = label;
    btn.title = title;
    btn.style.cssText = `
      border: none; background: transparent; color: inherit; cursor: pointer;
      width: 28px; height: 26px; border-radius: 4px; font-size: 14px;
      line-height: 1; padding: 0;
    `;
    btn.addEventListener("mouseenter", () => {
      btn.style.background = colors.hover;
    });
    btn.addEventListener("mouseleave", () => {
      btn.style.background = "transparent";
    });
    btn.addEventListener("click", onClick);
    bar.appendChild(btn);
    return btn;
  }

  function addSeparator() {
    const sep = document.createElement("span");
    sep.style.cssText = `width: 1px; height: 18px; background: ${colors.border}; margin: 0 4px;`;
    bar.appendChild(sep);
  }

  addButton("←", "Back", () => window.history.back());
  addButton("→", "Forward", () => window.history.forward());
  addButton("⟳", "Reload", () => window.location.reload());
  addButton("⌂", "Home", () => window.pakeGoHome && window.pakeGoHome());
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

  document.body.appendChild(bar);
  // Push the page below the bar. Sites with position:fixed headers pinned to
  // top:0 will sit underneath it; the toolbar is opt-in for that reason.
  document.documentElement.style.setProperty(
    "margin-top",
    `${BAR_HEIGHT}px`,
    "important",
  );
});

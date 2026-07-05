// Runs inside each content-tab webview when tabbed mode is on. Reports the
// tab's current title and URL up to Rust so the tab strip can label it, and
// keeps reporting as the page navigates (YouTube is a SPA, so the URL/title
// change without a full reload).
(function () {
  if (window.pakeConfig && window.pakeConfig.tabs !== true) return;

  function report() {
    const invoke = window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;
    const label = window.__PAKE_TAB_LABEL__;
    if (!invoke || !label) return;
    invoke("tab_report", {
      label,
      title: document.title || location.href,
      url: location.href,
    }).catch(() => {});
  }

  let lastKey = "";
  function reportIfChanged() {
    const key = (document.title || "") + "|" + location.href;
    if (key !== lastKey) {
      lastKey = key;
      report();
    }
  }

  function start() {
    reportIfChanged();
    // Observe <title> mutations for SPA title swaps.
    try {
      const titleEl = document.querySelector("title");
      if (titleEl) {
        new MutationObserver(reportIfChanged).observe(titleEl, {
          childList: true,
        });
      }
    } catch (e) {}
    window.addEventListener("popstate", reportIfChanged);
    window.addEventListener("hashchange", reportIfChanged);
    window.addEventListener("pageshow", reportIfChanged);
    // Fallback poll: SPA route changes via pushState don't fire the events above.
    setInterval(reportIfChanged, 1000);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", start);
  } else {
    start();
  }
})();

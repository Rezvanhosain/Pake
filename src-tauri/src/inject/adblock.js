// Lightweight, opt-in ad/tracker request blocking (adblock_mode: off|basic|strict).
// This is JS-level blocking of fetch/XHR/DOM element requests against a small
// curated hostname list, not a cosmetic filter engine or full blocker. Runs
// before page scripts so it must be first in the injection-script chain.
(() => {
  const mode = window.pakeConfig?.adblock_mode;
  if (mode !== "basic" && mode !== "strict") return;

  const BASIC_HOSTS = [
    "doubleclick.net",
    "googlesyndication.com",
    "googleadservices.com",
    "adnxs.com",
    "adsrvr.org",
    "taboola.com",
    "outbrain.com",
    "criteo.com",
    "pubmatic.com",
    "rubiconproject.com",
    "openx.net",
    "moatads.com",
    "scorecardresearch.com",
    "quantserve.com",
    "media.net",
  ];
  const STRICT_HOSTS = [
    "google-analytics.com",
    "googletagmanager.com",
    "connect.facebook.net",
    "hotjar.com",
    "mixpanel.com",
    "segment.io",
    "amplitude.com",
    "fullstory.com",
    "clarity.ms",
  ];
  const hosts =
    mode === "strict" ? BASIC_HOSTS.concat(STRICT_HOSTS) : BASIC_HOSTS;

  function isBlocked(url) {
    try {
      const hostname = new URL(url, window.location.href).hostname;
      return hosts.some((h) => hostname === h || hostname.endsWith("." + h));
    } catch {
      return false;
    }
  }

  const originalFetch = window.fetch;
  window.fetch = function (input, init) {
    const url = typeof input === "string" ? input : input?.url;
    if (url && isBlocked(url)) {
      return Promise.resolve(new Response("", { status: 204 }));
    }
    return originalFetch.call(this, input, init);
  };

  const originalOpen = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (method, url, ...rest) {
    this.__pakeBlocked = typeof url === "string" && isBlocked(url);
    return originalOpen.call(this, method, url, ...rest);
  };
  const originalSend = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.send = function (...args) {
    if (this.__pakeBlocked) return;
    return originalSend.apply(this, args);
  };

  // Catch script/img/iframe tags loaded straight from parsed HTML, not just
  // JS-initiated requests, without needing a MutationObserver on every node.
  for (const tag of ["script", "img", "iframe"]) {
    const proto =
      tag === "script"
        ? HTMLScriptElement.prototype
        : tag === "img"
          ? HTMLImageElement.prototype
          : HTMLIFrameElement.prototype;
    const descriptor = Object.getOwnPropertyDescriptor(proto, "src");
    if (!descriptor?.set) continue;
    Object.defineProperty(proto, "src", {
      ...descriptor,
      set(value) {
        if (value && isBlocked(value)) return;
        descriptor.set.call(this, value);
      },
    });
  }

  // Catches src set via setAttribute() or parsed straight out of HTML, which
  // bypass the property-setter override above (createElement + `.src =`).
  function purgeIfBlocked(node) {
    if (node.nodeType !== 1) return;
    const tag = node.tagName;
    if (tag === "SCRIPT" || tag === "IMG" || tag === "IFRAME") {
      const src = node.getAttribute && node.getAttribute("src");
      if (src && isBlocked(src)) node.remove();
    }
  }
  new MutationObserver((mutations) => {
    for (const mutation of mutations) {
      for (const node of mutation.addedNodes) purgeIfBlocked(node);
    }
  }).observe(document.documentElement, { childList: true, subtree: true });
})();

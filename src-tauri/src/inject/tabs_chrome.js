// Tab strip UI. Runs inside the tiny top "pake-chrome" webview. Rust owns the
// tab list and pushes it here via the `tabs://state` event; clicks here call
// back into Rust (`tab_new`, `tab_switch`, `tab_close`).
(function () {
  function boot() {
    const tauri = window.__TAURI__;
    if (!tauri) {
      // Tauri API not ready yet; retry shortly.
      setTimeout(boot, 30);
      return;
    }
    const invoke = tauri.core.invoke;
    const listen = tauri.event.listen;

    const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
    const c = dark
      ? {
          bar: "#202124",
          tab: "#303134",
          active: "#3c4043",
          fg: "#e8eaed",
          sub: "#9aa0a6",
          hover: "#3c4043",
          border: "#000",
        }
      : {
          bar: "#dee1e6",
          tab: "#f1f3f4",
          active: "#ffffff",
          fg: "#202124",
          sub: "#5f6368",
          hover: "#e8eaed",
          border: "#c8ccd0",
        };

    document.documentElement.style.height = "100%";
    document.body.style.cssText =
      "margin:0;height:100%;overflow:hidden;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;";

    const bar = document.createElement("div");
    bar.style.cssText = `display:flex;align-items:flex-end;height:44px;background:${c.bar};padding:6px 6px 0 6px;box-sizing:border-box;gap:4px;overflow-x:auto;overflow-y:hidden;white-space:nowrap;`;
    document.body.appendChild(bar);

    const strip = document.createElement("div");
    strip.style.cssText =
      "display:flex;align-items:flex-end;gap:4px;flex:0 0 auto;";
    bar.appendChild(strip);

    const plus = document.createElement("button");
    plus.type = "button";
    plus.textContent = "+";
    plus.title = "New tab (Ctrl+T)";
    plus.style.cssText = `flex:0 0 auto;border:none;background:transparent;color:${c.fg};font-size:20px;line-height:1;width:32px;height:32px;margin-bottom:2px;border-radius:8px;cursor:pointer;`;
    plus.addEventListener(
      "mouseenter",
      () => (plus.style.background = c.hover),
    );
    plus.addEventListener(
      "mouseleave",
      () => (plus.style.background = "transparent"),
    );
    plus.addEventListener("click", () => invoke("tab_new", {}));
    bar.appendChild(plus);

    // "Restore previous session on startup" toggle. Pushed to the right edge of
    // the strip. Reflects the persisted setting and flips it on click.
    const gear = document.createElement("button");
    gear.type = "button";
    gear.textContent = "⟳";
    gear.style.cssText = `flex:0 0 auto;margin-left:auto;border:none;background:transparent;color:${c.fg};font-size:16px;line-height:1;width:32px;height:32px;margin-bottom:2px;border-radius:8px;cursor:pointer;`;
    gear.addEventListener(
      "mouseenter",
      () => (gear.style.background = c.hover),
    );
    gear.addEventListener(
      "mouseleave",
      () => (gear.style.background = "transparent"),
    );
    let restoreOn = true;
    function paintGear() {
      gear.style.opacity = restoreOn ? "1" : "0.4";
      gear.title =
        "Restore previous session on startup: " + (restoreOn ? "On" : "Off");
    }
    Promise.resolve(invoke("session_get_restore", {}))
      .then((v) => {
        restoreOn = !!v;
        paintGear();
      })
      .catch(() => paintGear());
    gear.addEventListener("click", () => {
      restoreOn = !restoreOn;
      paintGear();
      invoke("session_set_restore", { enabled: restoreOn });
    });
    bar.appendChild(gear);

    function faviconFor(url) {
      try {
        const u = new URL(url);
        return `https://www.google.com/s2/favicons?domain=${u.hostname}&sz=32`;
      } catch (e) {
        return "";
      }
    }

    function render(state) {
      strip.innerHTML = "";
      const tabs = (state && state.tabs) || [];
      const active = (state && state.active) || "";
      tabs.forEach((t) => {
        const isActive = t.label === active;
        const tab = document.createElement("div");
        tab.title = t.title || t.url || "";
        tab.style.cssText = `display:flex;align-items:center;gap:6px;height:34px;min-width:120px;max-width:220px;padding:0 8px 0 10px;border-radius:9px 9px 0 0;cursor:default;box-sizing:border-box;background:${
          isActive ? c.active : c.tab
        };color:${isActive ? c.fg : c.sub};font-size:12px;`;

        const fav = document.createElement("img");
        fav.src = faviconFor(t.url);
        fav.width = 16;
        fav.height = 16;
        fav.style.cssText = "flex:0 0 auto;border-radius:3px;";
        fav.addEventListener("error", () => (fav.style.visibility = "hidden"));
        tab.appendChild(fav);

        const label = document.createElement("span");
        label.textContent = t.title || t.url || "Loading…";
        label.style.cssText =
          "flex:1 1 auto;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;";
        tab.appendChild(label);

        const close = document.createElement("span");
        close.textContent = "✕";
        close.title = "Close tab";
        close.style.cssText = `flex:0 0 auto;width:18px;height:18px;line-height:18px;text-align:center;border-radius:50%;font-size:11px;color:${c.sub};`;
        close.addEventListener(
          "mouseenter",
          () => (close.style.background = c.hover),
        );
        close.addEventListener(
          "mouseleave",
          () => (close.style.background = "transparent"),
        );
        close.addEventListener("click", (e) => {
          e.stopPropagation();
          invoke("tab_close", { label: t.label });
        });

        tab.addEventListener("click", () =>
          invoke("tab_switch", { label: t.label }),
        );
        // Middle-click a tab to close it, matching browser convention.
        tab.addEventListener("auxclick", (e) => {
          if (e.button === 1) invoke("tab_close", { label: t.label });
        });
        tab.appendChild(close);
        strip.appendChild(tab);
      });
    }

    // Register the listener BEFORE asking for state, so the initial push is
    // never missed to a listen()-registration race.
    Promise.resolve(listen("tabs-state", (event) => render(event.payload)))
      .then(() => invoke("tab_ready", {}))
      .catch(() => invoke("tab_ready", {}));
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", boot);
  } else {
    boot();
  }
})();

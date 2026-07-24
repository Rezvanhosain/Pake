import fs from "fs";
import path from "path";
import { runInNewContext } from "node:vm";
import { describe, expect, it } from "vitest";

// Load event.js into a fake DOM rich enough to build and inspect the custom
// context menu, so we can drive right-clicks on links and linked media and
// assert which "open in new tab" items appear and what they invoke.
function loadEvent({
  tabs = true,
  locationHref = "https://www.youtube.com/",
} = {}) {
  const source = fs.readFileSync(
    path.join(process.cwd(), "src-tauri/src/inject/event.js"),
    "utf-8",
  );

  const invokeCalls = [];
  const invoke = (command, payload) => {
    invokeCalls.push([command, payload]);
    return Promise.resolve();
  };
  const docListeners = {};
  const elementsById = new Map();

  function createElement(tagName = "div") {
    const el = {
      tagName: tagName.toUpperCase(),
      style: {},
      children: [],
      listeners: {},
      textContent: "",
      _id: undefined,
      setAttribute() {},
      addEventListener(type, handler) {
        (this.listeners[type] = this.listeners[type] || []).push(handler);
      },
      appendChild(child) {
        this.children.push(child);
        if (child._id) elementsById.set(child._id, child);
        return child;
      },
      removeChild(child) {
        this.children = this.children.filter((c) => c !== child);
      },
      remove() {
        if (this._id) elementsById.delete(this._id);
      },
      getBoundingClientRect() {
        return { right: 100, bottom: 100, width: 120, height: 80 };
      },
      set id(v) {
        this._id = v;
        elementsById.set(v, this);
      },
      get id() {
        return this._id;
      },
    };
    return el;
  }

  const body = createElement("body");

  const context = {
    console,
    URL,
    Event: class {},
    Notification: function () {},
    setTimeout,
    clearTimeout,
    navigator: {
      userAgent: "Mozilla/5.0",
      language: "en-US",
      clipboard: { writeText: () => Promise.resolve() },
    },
    window: {
      innerWidth: 1200,
      innerHeight: 800,
      matchMedia: () => ({ matches: false }),
      localStorage: { getItem: () => null, setItem: () => {} },
      location: { href: locationHref, origin: "https://www.youtube.com" },
      addEventListener: () => {},
      removeEventListener: () => {},
      open: () => ({}),
      isAuthLink: () => false,
      isAuthPopup: () => false,
      pakeConfig: { tabs },
      __TAURI__: {
        core: { invoke },
        window: {
          getCurrentWindow: () => ({
            startDragging: () => {},
            isFullscreen: () => Promise.resolve(false),
            setFullscreen: () => {},
          }),
        },
      },
    },
    document: {
      addEventListener(type, handler) {
        (docListeners[type] = docListeners[type] || []).push(handler);
      },
      removeEventListener() {},
      createElement,
      getElementById: (id) => elementsById.get(id) || null,
      getElementsByTagName: () => [{ style: {} }],
      body,
      execCommand: () => {},
    },
  };
  context.window.navigator = context.navigator;
  context.window.window = context.window;

  runInNewContext(source, context);

  // Fire DOMContentLoaded so the menu wiring installs.
  (docListeners.DOMContentLoaded || []).forEach((h) => h());
  const contextmenu = (docListeners.contextmenu || []).find(
    (h) => h.length >= 1,
  );

  function fireContextMenu(target) {
    let prevented = false;
    contextmenu({
      target,
      clientX: 10,
      clientY: 10,
      preventDefault() {
        prevented = true;
      },
      stopPropagation() {},
    });
    const menu = elementsById.get("pake-context-menu");
    return { prevented, menu };
  }

  function anchor(href) {
    return { tagName: "A", href, style: {} };
  }
  function linkTarget(href) {
    // A plain <a> element right-clicked directly.
    const a = anchor(href);
    a.closest = (sel) => (sel === "a" ? a : null);
    return a;
  }
  function imgInLink(imgSrc, href) {
    // An <img> (e.g. a YouTube thumbnail) nested inside an <a href>.
    const a = anchor(href);
    return {
      tagName: "IMG",
      src: imgSrc,
      style: {},
      closest: (sel) => (sel === "a" ? a : null),
    };
  }
  function plainTarget() {
    return { tagName: "DIV", style: {}, closest: () => null };
  }

  function itemTexts(menu) {
    return menu ? menu.children.map((c) => c.textContent) : [];
  }
  function clickItem(menu, text) {
    const item = menu.children.find((c) => c.textContent === text);
    (item.listeners.click || []).forEach((h) =>
      h({ preventDefault() {}, stopPropagation() {} }),
    );
  }

  return {
    fireContextMenu,
    linkTarget,
    imgInLink,
    plainTarget,
    itemTexts,
    clickItem,
    invokeCalls,
  };
}

describe("new-tab context menu (event.js)", () => {
  it("offers 'Open link in new tab' on a plain https link and opens a tab", () => {
    const ctx = loadEvent();
    const { prevented, menu } = ctx.fireContextMenu(
      ctx.linkTarget("https://example.com/page"),
    );
    expect(prevented).toBe(true);
    expect(ctx.itemTexts(menu)).toContain("Open link in new tab");
    ctx.clickItem(menu, "Open link in new tab");
    expect(ctx.invokeCalls).toContainEqual([
      "tab_new",
      { url: "https://example.com/page" },
    ]);
  });

  it("offers 'Open video in new tab' on a YouTube thumbnail (img inside /watch link)", () => {
    const ctx = loadEvent();
    const { prevented, menu } = ctx.fireContextMenu(
      ctx.imgInLink(
        "https://i.ytimg.com/vi/abc123/hq.jpg",
        "https://www.youtube.com/watch?v=abc123",
      ),
    );
    expect(prevented).toBe(true);
    const texts = ctx.itemTexts(menu);
    expect(texts).toContain("Open video in new tab");
    // Media actions are still preserved.
    expect(texts).toContain("Download Image");
    ctx.clickItem(menu, "Open video in new tab");
    expect(ctx.invokeCalls).toContainEqual([
      "tab_new",
      { url: "https://www.youtube.com/watch?v=abc123" },
    ]);
  });

  it("does not offer a new-tab item for javascript: links", () => {
    const ctx = loadEvent();
    const { menu } = ctx.fireContextMenu(ctx.linkTarget("javascript:void(0)"));
    // A menu may still show (Copy Address etc.) but no new-tab action.
    const texts = ctx.itemTexts(menu);
    expect(texts).not.toContain("Open link in new tab");
    expect(texts).not.toContain("Open video in new tab");
  });

  it("omits the new-tab item when tabbed mode is off", () => {
    const ctx = loadEvent({ tabs: false });
    const { menu } = ctx.fireContextMenu(
      ctx.linkTarget("https://example.com/page"),
    );
    expect(ctx.itemTexts(menu)).not.toContain("Open link in new tab");
  });

  it("leaves the native menu alone for non-link, non-media targets", () => {
    const ctx = loadEvent();
    const { prevented, menu } = ctx.fireContextMenu(ctx.plainTarget());
    expect(prevented).toBe(false);
    expect(menu).toBeFalsy();
  });
});

# VoxLink Mobile Usability Fix — Design

> **For Claude:** REQUIRED SUB-SKILL: Use writing-plans to create the implementation plan from this design.

**Goal:** Make the VoxLink web client usable on mobile phones — fix the critical blockers that prevent mobile testers from using the site. No new features; just make the existing desktop UI work on touch screens at narrow widths.

**Architecture:** Pure CSS media queries + minimal JS for the hamburger toggle and reaction-picker clamping. No layout redesign, no bottom tab bar, no new components. The existing `@media (max-width: 480px)` sidebar-as-drawer CSS is half-implemented — it hides the sidebar but provides no way to open it. This design completes that wiring and fixes the remaining touch/narrow-width issues.

**Tech Stack:** Vanilla JS, CSS, no build step. Files: `web/index.html`, `web/style.css`, `web/app.js`.

---

## 1. Hamburger menu + sidebar drawer

**The critical blocker:** `@media (max-width: 480px)` hides `#sidebar` (line 736 of style.css) and shows it as a fixed overlay when `.open` (line 737) — but there is no button to toggle `.open`, and no JS to do so. Mobile users cannot access channels, members, voice, profile, or logout.

### HTML (index.html)
Add a hamburger button (☰) as the **first child** of `#channel-header` (line 91):
```html
<button id="hamburger-btn" class="icon-btn hamburger-btn" title="Open menu" aria-label="Open menu">☰</button>
```
It is `display: none` by default (desktop) and `display: flex` in the mobile media query.

Add a backdrop div **after** `#sidebar` (inside `#chat-screen`, after line 87):
```html
<div id="sidebar-backdrop" class="sidebar-backdrop"></div>
```
This is a transparent overlay that sits between the open drawer and the main area; tapping it closes the sidebar.

### JS (app.js)
In `bindEvents()`, add:
```js
  // Hamburger menu (mobile only — button is hidden on desktop)
  const hamburger = $('hamburger-btn');
  if (hamburger) {
    hamburger.addEventListener('click', e => {
      e.stopPropagation();
      $('sidebar').classList.toggle('open');
      $('sidebar-backdrop').classList.toggle('show');
    });
  }
  const backdrop = $('sidebar-backdrop');
  if (backdrop) {
    backdrop.addEventListener('click', () => {
      $('sidebar').classList.remove('open');
      backdrop.classList.remove('show');
    });
  }
```
No desktop guard needed — the button is `display: none` on desktop so it never receives clicks.

### CSS (style.css)
```css
/* Hamburger — hidden on desktop, shown on mobile */
.hamburger-btn { display: none; font-size: 20px; }
.sidebar-backdrop {
  display: none; position: fixed; inset: 0; z-index: 99;
  background: rgba(0,0,0,0.5);
}
.sidebar-backdrop.show { display: block; }
```
The existing `#sidebar.open` rule (line 737) already styles the drawer. The backdrop gets `z-index: 99` (below the sidebar's `z-index: 100`).

---

## 2. Touch-sized targets + input bar

### Touch targets
In the `@media (max-width: 480px)` block, bump small buttons to 40px minimum:
```css
  .icon-btn { width: 40px; height: 40px; font-size: 20px; }
  .send-btn { width: 40px; height: 40px; font-size: 20px; }
  .attach-btn { width: 36px; height: 36px; font-size: 24px; }
  .voice-join-btn { padding: 6px 12px; font-size: 13px; }
```

### Input bar
The `#input-box` is a flex row that already shrinks correctly. Two fixes:
- **Placeholder position:** `#input-placeholder { left: 60px; }` assumes desktop button sizes. On mobile with the larger attach button, bump to `left: 64px`.
- **Font size:** iOS Safari auto-zooms the page if a focusable input has font-size < 16px. The `contenteditable` is 15px — bump to 16px in the media query:
```css
  .chat-input { font-size: 16px; }
```

---

## 3. Modals + inspect card on mobile

### Profile modal
`.modal-card` (line 579-588) has `width: 460px; max-width: calc(100vw - 40px)` — the max-width already adapts. But there's no `max-height`, so a tall modal (avatar + username + about + appearance pickers) overflows off-screen on short viewports. Add to the media query:
```css
  .modal-card { max-height: 90vh; overflow-y: auto; }
```

### Inspect card
The existing `@media (max-width: 480px)` sets `#inspect-card { left: 8px; right: 8px; width: auto; }` — that's correct. Verify it sits above the sidebar drawer (the drawer is `z-index: 100`; the inspect card is `z-index: 50` per line 525 — the drawer should cover it, which is fine since you wouldn't open both simultaneously).

---

## 4. Reaction picker on mobile

`showReactionPicker` (app.js) positions the picker at `clientX/clientY` from the `contextmenu` event (long-press on mobile). On a narrow screen, the picker can overflow the right/bottom edge. Fix: clamp the position in `showReactionPicker`:
```js
  // Clamp to viewport so the picker doesn't overflow on mobile
  const pw = 220, ph = 44; // approximate picker dimensions (6 emojis)
  const maxX = window.innerWidth - pw - 8;
  const maxY = window.innerHeight - ph - 8;
  picker.style.left = `${Math.min(x, maxX)}px`;
  picker.style.top  = `${Math.min(y, maxY)}px`;
```

---

## 5. Misc mobile fixes

### Smooth scrolling on iOS
Add `-webkit-overflow-scrolling: touch` to `#messages` and `#sidebar-scroll`:
```css
#messages { -webkit-overflow-scrolling: touch; }
#sidebar-scroll { -webkit-overflow-scrolling: touch; }
```
(These go in the base CSS, not the media query — they're harmless on desktop.)

### Dynamic viewport height (100dvh)
Mobile browsers' URL bars make `100vh` taller than the visible area, hiding the input bar. Use `100dvh` (dynamic viewport height) where supported, with `100vh` fallback:
```css
.screen { height: 100vh; height: 100dvh; }
#sidebar { height: 100vh; height: 100dvh; }
#main-area { height: 100vh; height: 100dvh; }
```
The second declaration overrides the first only if the browser supports `dvh`; older browsers ignore it and use `100vh`.

---

## 6. Scope boundaries

**In scope:** hamburger menu + sidebar drawer, backdrop tap-to-close, touch-sized buttons, input font-size fix, modal max-height + scroll, reaction picker viewport clamping, smooth scroll, 100dvh.

**Out of scope:** bottom tab bar, swipe gestures, mobile-native layout redesign, new features, native app changes.

**Constraints honored:** no build step (web stays static ES module + CSS), no new dependencies, no JS framework, cross-OS (works on any mobile browser including Chromebook tablets).

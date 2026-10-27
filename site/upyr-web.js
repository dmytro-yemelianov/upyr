// Minimal DOM facade over the upyr-wasm session, following the contract in
// docs/architecture/wasm-web-plan.md ("Browser event contract" and
// "Definition of done"). This is a site-local implementation of the still
//-unpublished `packages/upyr-web` facade, scoped to plain
// `<input type="text">` / `<textarea>` fields for this site's live demo.
//
// It never attaches to password, current/new-password autocomplete,
// OTP/numeric, readonly, disabled, or `data-upyr="off"` fields.

const EXCLUDED_AUTOCOMPLETE = new Set([
  "current-password",
  "new-password",
  "one-time-code",
]);

const NAVIGATION_KEYS = new Set([
  "ArrowLeft",
  "ArrowRight",
  "ArrowUp",
  "ArrowDown",
  "Home",
  "End",
  "PageUp",
  "PageDown",
]);

/**
 * Field-level heuristics for the fields Upyr must never observe or touch:
 * password fields, password-autofill/OTP-autofill fields, numeric/OTP entry
 * fields, and fields explicitly opted out or disabled/read-only.
 */
export function isExcludedField(element) {
  if (element.dataset.upyr === "off") return true;
  if (element.disabled || element.readOnly) return true;

  const type = (element.type || "text").toLowerCase();
  if (type === "password" || type === "number") return true;

  const autocomplete = (element.getAttribute("autocomplete") || "").toLowerCase();
  if (EXCLUDED_AUTOCOMPLETE.has(autocomplete)) return true;

  const inputMode = (element.inputMode || "").toLowerCase();
  if (inputMode === "numeric") return true;

  return false;
}

/** One field's live correction state: the session, pending decision, and DOM listeners. */
class UpyrFieldSession {
  constructor(element, session, onDecision) {
    this.element = element;
    this.session = session;
    this.onDecision = onDecision || (() => {});
    this.pending = null;
    this.destroyed = false;

    this.handleKeyDown = this.handleKeyDown.bind(this);
    this.handleInput = this.handleInput.bind(this);
    this.handleNavigationKeyUp = this.handleNavigationKeyUp.bind(this);
    this.handleResetEvent = this.handleResetEvent.bind(this);

    element.addEventListener("keydown", this.handleKeyDown);
    element.addEventListener("input", this.handleInput);
    element.addEventListener("keyup", this.handleNavigationKeyUp);
    element.addEventListener("click", this.handleResetEvent);
    element.addEventListener("blur", this.handleResetEvent);
    element.addEventListener("paste", this.handleResetEvent);
    element.addEventListener("drop", this.handleResetEvent);
    element.addEventListener("cut", this.handleResetEvent);
    element.addEventListener("compositionstart", this.handleResetEvent);
  }

  destroy() {
    if (this.destroyed) return;
    this.destroyed = true;
    this.element.removeEventListener("keydown", this.handleKeyDown);
    this.element.removeEventListener("input", this.handleInput);
    this.element.removeEventListener("keyup", this.handleNavigationKeyUp);
    this.element.removeEventListener("click", this.handleResetEvent);
    this.element.removeEventListener("blur", this.handleResetEvent);
    this.element.removeEventListener("paste", this.handleResetEvent);
    this.element.removeEventListener("drop", this.handleResetEvent);
    this.element.removeEventListener("cut", this.handleResetEvent);
    this.element.removeEventListener("compositionstart", this.handleResetEvent);
  }

  handleResetEvent() {
    this.pending = null;
    this.session.reset();
  }

  handleNavigationKeyUp(event) {
    if (NAVIGATION_KEYS.has(event.key)) {
      this.handleResetEvent();
    }
  }

  handleKeyDown(event) {
    const decision = this.processKeyDown({
      code: event.code,
      key: event.key,
      shiftKey: event.shiftKey,
      capsLock: event.getModifierState ? event.getModifierState("CapsLock") : false,
      ctrlKey: event.ctrlKey,
      altKey: event.altKey,
      metaKey: event.metaKey,
      altGraphKey: event.getModifierState ? event.getModifierState("AltGraph") : false,
      isComposing: event.isComposing,
    });
    return decision;
  }

  /**
   * Feeds one normalized keydown-shaped input into the session. Used both by
   * the real `keydown` listener above and by a virtual/on-screen keyboard,
   * which has a well-defined physical `code` per key even though it never
   * fires a native `KeyboardEvent`.
   */
  processKeyDown(input) {
    const decision = this.session.keyDown(input);
    this.pending = decision.applyAfterInput ? decision : null;
    this.onDecision(decision);
    return decision;
  }

  handleInput(event) {
    if (event.inputType && event.inputType.startsWith("history")) {
      this.handleResetEvent();
      return;
    }
    const pending = this.pending;
    this.pending = null;
    if (!pending) return;
    this.applyPending(pending);
  }

  applyPending(decision) {
    const element = this.element;
    const caret = element.selectionStart;
    if (caret === null || caret !== element.selectionEnd) return;
    const before = element.value.slice(0, caret);
    if (!before.endsWith(decision.expectedSource)) return;

    const start = caret - decision.expectedSource.length;
    element.setRangeText(decision.replacement, start, caret, "end");
    element.dispatchEvent(new Event("input", { bubbles: true }));
  }
}

/**
 * Attaches a live Upyr correction session to a text field. Returns `null`
 * for excluded fields (password/OTP/disabled/opted-out); otherwise returns a
 * controller with `reset()`, `destroy()`, and `simulateKeyDown()` for a
 * virtual keyboard to feed physical-key events directly.
 */
export function attach(element, session, options) {
  if (isExcludedField(element)) return null;
  const onDecision = options && options.onDecision;
  const fieldSession = new UpyrFieldSession(element, session, onDecision);
  return {
    reset: () => fieldSession.handleResetEvent(),
    destroy: () => fieldSession.destroy(),
    simulateKeyDown: (input) => fieldSession.processKeyDown(input),
  };
}

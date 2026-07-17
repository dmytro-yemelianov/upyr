import init, { UpyrSession } from "./wasm/upyr_wasm.js";
import { attach, isExcludedField } from "./upyr-web.js";
import { buildKeyboard } from "./keyboard.js";

// A scripted sequence of physical-key codes that spells known EN/UK
// wrong-layout pairs (see README and upyr-wasm's own tests), alternating
// direction on every word so the layout visibly flips back and forth.
const INTENSE_SCRIPT = [
  { layout: "english", codes: ["KeyG", "KeyH", "KeyB", "KeyD", "KeyS", "KeyN", "Space"] }, // ghbdsn -> привіт
  { layout: "ukrainian", codes: ["KeyH", "KeyE", "KeyL", "KeyL", "KeyO", "Space"] }, // руддщ -> hello
  { layout: "english", codes: ["BracketLeft", "KeyK", "KeyS", "Comma", "Space"] }, // [ks, -> хліб
  { layout: "ukrainian", codes: ["KeyC", "KeyO", "KeyD", "KeyE", "KeyX", "Space"] }, // send as-is example
];

function statusText(decision) {
  switch (decision.kind) {
    case "correct":
      return `Corrected ${decision.expectedSource.trim()} → ${decision.replacement.trim()} (${decision.direction})`;
    case "suggest":
      return `Would suggest ${decision.expectedSource.trim()} → ${decision.replacement.trim()}`;
    case "wait":
      return "Tracking…";
    case "reset":
      return `Reset (${decision.reason})`;
    default:
      return "";
  }
}

async function main() {
  const root = document.getElementById("live-demo");
  if (!root) return;

  const keyboardHost = root.querySelector("[data-role='keyboard']");
  const textField = root.querySelector("[data-role='demo-field']");
  const passwordField = root.querySelector("[data-role='password-field']");
  const statusLine = root.querySelector("[data-role='status']");
  const layoutBadge = root.querySelector("[data-role='layout-badge']");
  const manualButtons = root.querySelectorAll("[data-manual-layout]");
  const autoButton = root.querySelector("[data-role='auto-toggle']");
  const intenseButton = root.querySelector("[data-role='intense-toggle']");
  const excludedNote = root.querySelector("[data-role='excluded-note']");

  await init();

  let autoFollow = true;
  let intenseRunning = false;
  let intenseTimer = null;

  const session = new UpyrSession({ mode: "auto", sourceLayout: "english" });

  function setStatus(message) {
    if (statusLine) statusLine.textContent = message;
  }

  function setLayout(layout, { manual } = {}) {
    keyboard.setLayout(layout);
    session.setSourceLayout(layout);
    if (layoutBadge) {
      layoutBadge.textContent = layout === "ukrainian" ? "УК" : "EN";
    }
    for (const button of manualButtons) {
      button.classList.toggle("is-active", button.dataset.manualLayout === layout);
    }
    if (manual) {
      autoFollow = false;
      if (autoButton) autoButton.classList.remove("is-active");
    }
  }

  const keyboard = buildKeyboard(keyboardHost, ({ code, char }) => {
    const decision = fieldSession
      ? fieldSession.simulateKeyDown({
          code,
          key: char ?? code,
          shiftKey: false,
          capsLock: false,
          ctrlKey: false,
          altKey: false,
          metaKey: false,
          altGraphKey: false,
          isComposing: false,
        })
      : null;

    if (code === "Backspace") {
      const caret = textField.selectionStart;
      if (caret !== null && caret > 0 && caret === textField.selectionEnd) {
        textField.setRangeText("", caret - 1, caret, "end");
        textField.dispatchEvent(new Event("input", { bubbles: true }));
      }
    } else if (char) {
      const caret = textField.selectionStart ?? textField.value.length;
      textField.setRangeText(char, caret, textField.selectionEnd ?? caret, "end");
      textField.dispatchEvent(new Event("input", { bubbles: true }));
    }

    if (decision) setStatus(statusText(decision));
    if (decision && autoFollow && decision.targetLayout) {
      setLayout(decision.targetLayout);
    }
  });

  const fieldSession = attach(textField, session, {
    onDecision: (decision) => {
      setStatus(statusText(decision));
      if (autoFollow && decision.targetLayout) {
        setLayout(decision.targetLayout);
      }
    },
  });

  // Proves the field-exclusion contract live: this is a real password input,
  // and `attach` refuses it, exactly like docs/architecture/wasm-web-plan.md
  // requires ("never attach password... fields").
  if (passwordField && excludedNote) {
    const passwordSession = attach(passwordField, session);
    excludedNote.textContent = passwordSession === null && isExcludedField(passwordField)
      ? "Upyr never attaches to this field (type=\"password\")."
      : "Warning: exclusion check failed.";
  }

  for (const button of manualButtons) {
    button.addEventListener("click", () => setLayout(button.dataset.manualLayout, { manual: true }));
  }

  if (autoButton) {
    autoButton.addEventListener("click", () => {
      autoFollow = !autoFollow;
      autoButton.classList.toggle("is-active", autoFollow);
    });
    autoButton.classList.toggle("is-active", autoFollow);
  }

  function stopIntenseDemo() {
    intenseRunning = false;
    if (intenseTimer) window.clearTimeout(intenseTimer);
    intenseTimer = null;
    if (intenseButton) intenseButton.textContent = "Play intense switching demo";
  }

  async function runIntenseDemo() {
    textField.value = "";
    textField.focus();
    fieldSession?.reset();
    for (const word of INTENSE_SCRIPT) {
      if (!intenseRunning) return;
      setLayout(word.layout);
      for (const code of word.codes) {
        if (!intenseRunning) return;
        keyboard.pressCode(code);
        await new Promise((resolve) => {
          intenseTimer = window.setTimeout(resolve, code === "Space" ? 420 : 110);
        });
      }
    }
    stopIntenseDemo();
  }

  if (intenseButton) {
    intenseButton.addEventListener("click", () => {
      if (intenseRunning) {
        stopIntenseDemo();
        return;
      }
      intenseRunning = true;
      intenseButton.textContent = "Stop demo";
      runIntenseDemo();
    });
  }

  setLayout("english");
  setStatus("Type on the keyboard above, or your real keyboard, in the field below.");
}

main().catch((error) => {
  console.error("Upyr live demo failed to start", error);
  const root = document.getElementById("live-demo");
  const statusLine = root && root.querySelector("[data-role='status']");
  if (statusLine) statusLine.textContent = "Live demo unavailable in this browser.";
});

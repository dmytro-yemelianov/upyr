// A visual, interactive on-screen Apple-style keyboard for the live demo.
// Each key corresponds to one physical position from Upyr's own EN/UK
// mapping (crates/upyr-core/src/layout.rs), so clicking a key produces
// exactly the character a real EN or УК input source would send from that
// physical position, and feeds the matching KeyboardEvent.code into the
// attached Upyr session.

// (english unshifted, ukrainian unshifted, english shifted, ukrainian shifted)
const PHYSICAL_KEYS = [
  ["Backquote", "`", "'", "~", "₴"],
  ["KeyQ", "q", "й", "Q", "Й"],
  ["KeyW", "w", "ц", "W", "Ц"],
  ["KeyE", "e", "у", "E", "У"],
  ["KeyR", "r", "к", "R", "К"],
  ["KeyT", "t", "е", "T", "Е"],
  ["KeyY", "y", "н", "Y", "Н"],
  ["KeyU", "u", "г", "U", "Г"],
  ["KeyI", "i", "ш", "I", "Ш"],
  ["KeyO", "o", "щ", "O", "Щ"],
  ["KeyP", "p", "з", "P", "З"],
  ["BracketLeft", "[", "х", "{", "Х"],
  ["BracketRight", "]", "ї", "}", "Ї"],
  ["Backslash", "\\", "ґ", "|", "Ґ"],
  ["KeyA", "a", "ф", "A", "Ф"],
  ["KeyS", "s", "і", "S", "І"],
  ["KeyD", "d", "в", "D", "В"],
  ["KeyF", "f", "а", "F", "А"],
  ["KeyG", "g", "п", "G", "П"],
  ["KeyH", "h", "р", "H", "Р"],
  ["KeyJ", "j", "о", "J", "О"],
  ["KeyK", "k", "л", "K", "Л"],
  ["KeyL", "l", "д", "L", "Д"],
  ["Semicolon", ";", "ж", ":", "Ж"],
  ["Quote", "'", "є", "\"", "Є"],
  ["KeyZ", "z", "я", "Z", "Я"],
  ["KeyX", "x", "ч", "X", "Ч"],
  ["KeyC", "c", "с", "C", "С"],
  ["KeyV", "v", "м", "V", "М"],
  ["KeyB", "b", "и", "B", "И"],
  ["KeyN", "n", "т", "N", "Т"],
  ["KeyM", "m", "ь", "M", "Ь"],
  ["Comma", ",", "б", "<", "Б"],
  ["Period", ".", "ю", ">", "Ю"],
  ["Slash", "/", ".", "?", ","],
];

const LETTER_CODES = new Set(
  PHYSICAL_KEYS.filter(([code]) => code.startsWith("Key")).map(([code]) => code),
);

const ROWS = [
  ["Backquote", "KeyQ", "KeyW", "KeyE", "KeyR", "KeyT", "KeyY", "KeyU", "KeyI", "KeyO", "KeyP", "BracketLeft", "BracketRight", "Backslash"],
  ["Tab", "KeyA", "KeyS", "KeyD", "KeyF", "KeyG", "KeyH", "KeyJ", "KeyK", "KeyL", "Semicolon", "Quote", "Return"],
  ["CapsLock", "KeyZ", "KeyX", "KeyC", "KeyV", "KeyB", "KeyN", "KeyM", "Comma", "Period", "Slash", "Backspace"],
  ["Control", "Option", "Command", "Space", "Command", "Option"],
];

const KEY_BY_CODE = new Map(PHYSICAL_KEYS.map((entry) => [entry[0], entry]));

const SPECIAL_LABELS = {
  Tab: "tab",
  Return: "return",
  CapsLock: "caps lock",
  Backspace: "delete",
  Control: "⌃",
  Option: "⌥",
  Command: "⌘",
  Space: "",
};

function labelFor(code, layout, shifted) {
  const entry = KEY_BY_CODE.get(code);
  if (!entry) return SPECIAL_LABELS[code] ?? code;
  const [, enPlain, ukPlain, enShift, ukShift] = entry;
  if (layout === "ukrainian") return shifted ? ukShift : ukPlain;
  return shifted ? enShift : enPlain;
}

/** Character this physical key sends given the active layout and modifiers. */
export function characterFor(code, layout, shiftActive, capsLockActive) {
  const entry = KEY_BY_CODE.get(code);
  if (!entry) return null;
  const shifted = LETTER_CODES.has(code) ? shiftActive !== capsLockActive : shiftActive;
  return labelFor(code, layout, shifted);
}

/**
 * Builds the on-screen keyboard inside `container` and wires key clicks to
 * `onKey({ code, char, shiftKey, capsLock })`. The caller owns turning that
 * into a real Upyr session decision and a real text insertion; this module
 * only knows about the visual keyboard and layout-dependent character table.
 */
export function buildKeyboard(container, onKey) {
  container.innerHTML = "";
  container.setAttribute("role", "group");
  container.setAttribute("aria-label", "On-screen Apple keyboard");

  let layout = "english";
  let shiftActive = false;
  let capsLockActive = false;
  const buttons = new Map();

  function refreshLabels() {
    for (const [code, button] of buttons) {
      if (!KEY_BY_CODE.has(code)) continue;
      const label = button.querySelector(".key-label");
      label.textContent = characterFor(code, layout, shiftActive, capsLockActive);
    }
    container.classList.toggle("is-ukrainian", layout === "ukrainian");
    container.classList.toggle("is-shifted", shiftActive);
    for (const button of container.querySelectorAll('[data-code="Shift"]')) {
      button.classList.toggle("is-active", shiftActive);
    }
    const capsButton = buttons.get("CapsLock");
    if (capsButton) capsButton.classList.toggle("is-active", capsLockActive);
  }

  function press(code, button) {
    if (button) {
      button.classList.add("is-pressed");
      window.setTimeout(() => button.classList.remove("is-pressed"), 110);
    }
    if (code === "Shift") {
      shiftActive = !shiftActive;
      refreshLabels();
      return;
    }
    if (code === "CapsLock") {
      capsLockActive = !capsLockActive;
      refreshLabels();
      return;
    }
    const char = code === "Space" ? " " : characterFor(code, layout, shiftActive, capsLockActive);
    onKey({ code, char, shiftKey: shiftActive, capsLock: capsLockActive });
    if (shiftActive) {
      // A single click acts like one held Shift chord, not a sticky modifier.
      shiftActive = false;
      refreshLabels();
    }
  }

  const DECORATIVE_CODES = new Set(["Control", "Option", "Command", "Tab", "Return"]);

  for (const row of ROWS) {
    const rowElement = document.createElement("div");
    rowElement.className = "keyboard-row";
    for (const code of row) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "key";
      button.dataset.code = code;
      if (["Tab", "Return", "CapsLock", "Backspace"].includes(code)) button.classList.add("key-wide");
      if (["Control", "Option", "Command"].includes(code)) button.classList.add("key-modifier");
      if (code === "Space") button.classList.add("key-space");
      if (code === "KeyF" || code === "KeyJ") button.classList.add("key-home");

      const label = document.createElement("span");
      label.className = "key-label";
      label.textContent = KEY_BY_CODE.has(code) ? labelFor(code, layout, false) : (SPECIAL_LABELS[code] ?? code);
      button.appendChild(label);

      if (DECORATIVE_CODES.has(code)) {
        button.disabled = true;
        button.classList.add("key-decorative");
        button.title = "Visual only in this demo";
      } else {
        button.addEventListener("click", () => press(code, button));
      }
      buttons.set(code, button);
      rowElement.appendChild(button);
    }
    container.appendChild(rowElement);
  }

  // A real Shift key, inserted at both ends of the third row, functional.
  const shiftLeft = document.createElement("button");
  shiftLeft.type = "button";
  shiftLeft.className = "key key-wide";
  shiftLeft.dataset.code = "Shift";
  shiftLeft.innerHTML = '<span class="key-label">⇧ shift</span>';
  shiftLeft.addEventListener("click", () => press("Shift", shiftLeft));
  const capsRow = container.querySelectorAll(".keyboard-row")[2];
  capsRow.insertBefore(shiftLeft, capsRow.firstChild);
  buttons.set("Shift", shiftLeft);

  const shiftRight = document.createElement("button");
  shiftRight.type = "button";
  shiftRight.className = "key key-wide";
  shiftRight.dataset.code = "Shift";
  shiftRight.innerHTML = '<span class="key-label">⇧ shift</span>';
  shiftRight.addEventListener("click", () => press("Shift", shiftRight));
  capsRow.appendChild(shiftRight);

  refreshLabels();

  return {
    setLayout(nextLayout) {
      layout = nextLayout;
      refreshLabels();
    },
    getLayout() {
      return layout;
    },
    pressCode(code) {
      press(code, buttons.get(code));
    },
    flash(code) {
      const button = buttons.get(code);
      if (!button) return;
      button.classList.add("is-pressed");
      window.setTimeout(() => button.classList.remove("is-pressed"), 110);
    },
  };
}

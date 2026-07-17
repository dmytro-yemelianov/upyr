#!/usr/bin/env node

const assert = require("node:assert/strict");
const path = require("node:path");

const bindingPath = process.argv[2];
if (!bindingPath) {
  throw new Error("usage: smoke_wasm_node.cjs <generated-binding.js>");
}

const upyr = require(path.resolve(bindingPath));
const conversion = upyr.convertText("ghbdsn", "english-to-ukrainian");
assert.deepEqual(conversion, {
  text: "привіт",
  direction: "english-to-ukrainian",
  changed: true,
});

const session = new upyr.UpyrSession({ sourceLayout: "english" });
let decision;
for (const code of ["KeyG", "KeyH", "KeyB", "KeyD", "KeyS", "KeyN", "Space"]) {
  decision = session.keyDown({
    code,
    key: code === "Space" ? " " : code.slice(-1).toLowerCase(),
    shiftKey: false,
    capsLock: false,
    ctrlKey: false,
    altKey: false,
    metaKey: false,
    altGraphKey: false,
    isComposing: false,
  });
}

assert.equal(decision.kind, "suggest");
assert.equal(decision.expectedSource, "ghbdsn ");
assert.equal(decision.replacement, "привіт ");
assert.equal(decision.applyAfterInput, true);
console.log("WASM/Node generated-binding smoke test passed");

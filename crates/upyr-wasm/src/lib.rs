#![deny(unsafe_code)]

//! Headless WebAssembly bindings for the portable Upyr correction engine.
//!
//! This crate deliberately does not touch the DOM. A browser host supplies
//! normalized `KeyboardEvent` data, verifies the returned `expectedSource`
//! suffix, and decides how suggestions or corrections are presented.

use serde::{Deserialize, Serialize};
use upyr_core::{
    AutoCorrectPolicy, AutoDecision, AutoWordTracker, Direction, InputLayout, PhysicalKey,
    PhysicalKeyEvent, Sensitivity, convert, convert_with_mapping, default_physical_mapping,
    evaluate,
};
use wasm_bindgen::prelude::*;

const MODEL_VERSION: &str = concat!("signed-ngram-v1-1m-", env!("UPYR_MODEL_SHA256_PREFIX"));

#[wasm_bindgen(typescript_custom_section)]
const TYPESCRIPT_TYPES: &str = r#"
export type UpyrDirection =
    | "english-to-ukrainian"
    | "ukrainian-to-english";
export type UpyrRequestedDirection = UpyrDirection | "smart";
export type UpyrLayout = "english" | "ukrainian";
export type UpyrSensitivity = "conservative" | "balanced" | "aggressive";

export interface UpyrMappingOverride {
    english: string;
    ukrainian: string;
    shiftedEnglish?: string;
    shiftedUkrainian?: string;
}

export interface UpyrSessionOptions {
    mode?: "suggest" | "auto";
    sensitivity?: UpyrSensitivity;
    minWordLength?: number;
    exceptions?: readonly string[];
    sourceLayout?: UpyrLayout;
    mappingOverrides?: readonly UpyrMappingOverride[];
}

export interface UpyrKeyDownInput {
    code: string;
    key: string;
    shiftKey: boolean;
    capsLock: boolean;
    ctrlKey: boolean;
    altKey: boolean;
    metaKey: boolean;
    altGraphKey: boolean;
    isComposing: boolean;
}

export interface UpyrPassiveDecision {
    kind: "wait" | "reset";
    reason: string;
    modelVersion: string;
    applyAfterInput: false;
    sourceLayout?: UpyrLayout;
}

export interface UpyrCorrectionDecision {
    kind: "suggest" | "correct";
    reason: "wrong-layout";
    modelVersion: string;
    applyAfterInput: true;
    sourceLayout: UpyrLayout;
    targetLayout: UpyrLayout;
    expectedSource: string;
    replacement: string;
    direction: UpyrDirection;
}

export type UpyrDecision = UpyrPassiveDecision | UpyrCorrectionDecision;

export interface UpyrConversionResult {
    text: string;
    direction: UpyrDirection;
    changed: boolean;
}
"#;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "UpyrSessionOptions")]
    pub type JsSessionOptions;

    #[wasm_bindgen(typescript_type = "UpyrKeyDownInput")]
    pub type JsKeyDownInput;

    #[wasm_bindgen(typescript_type = "UpyrLayout")]
    pub type JsLayout;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum SessionMode {
    Suggest,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MappingOverride {
    english: char,
    ukrainian: char,
    shifted_english: Option<char>,
    shifted_ukrainian: Option<char>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
struct SessionOptions {
    mode: SessionMode,
    sensitivity: Sensitivity,
    min_word_length: usize,
    exceptions: Vec<String>,
    source_layout: Option<InputLayout>,
    mapping_overrides: Vec<MappingOverride>,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            mode: SessionMode::Suggest,
            sensitivity: Sensitivity::Conservative,
            min_word_length: 4,
            exceptions: Vec::new(),
            source_layout: None,
            mapping_overrides: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KeyDownInput {
    code: String,
    key: String,
    shift_key: bool,
    caps_lock: bool,
    ctrl_key: bool,
    alt_key: bool,
    meta_key: bool,
    alt_graph_key: bool,
    is_composing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum DecisionKind {
    Wait,
    Reset,
    Suggest,
    Correct,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebDecision {
    kind: DecisionKind,
    reason: &'static str,
    model_version: &'static str,
    apply_after_input: bool,
    source_layout: Option<InputLayout>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_layout: Option<InputLayout>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    replacement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    direction: Option<Direction>,
}

impl WebDecision {
    fn passive(
        kind: DecisionKind,
        reason: &'static str,
        source_layout: Option<InputLayout>,
    ) -> Self {
        Self {
            kind,
            reason,
            model_version: MODEL_VERSION,
            apply_after_input: false,
            source_layout,
            target_layout: None,
            expected_source: None,
            replacement: None,
            direction: None,
        }
    }

    fn correction(
        kind: DecisionKind,
        source_layout: Option<InputLayout>,
        correction: upyr_core::AutoCorrection,
    ) -> Self {
        let target_layout = match correction.direction {
            Direction::EnglishToUkrainian => Some(InputLayout::Ukrainian),
            Direction::UkrainianToEnglish => Some(InputLayout::English),
            Direction::Smart => None,
        };
        Self {
            kind,
            reason: "wrong-layout",
            model_version: MODEL_VERSION,
            // A Space decision is produced on keydown, before the browser has
            // inserted that Space into the editable value.
            apply_after_input: true,
            source_layout,
            target_layout,
            expected_source: Some(correction.expected_source),
            replacement: Some(correction.replacement),
            direction: Some(correction.direction),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConversionResult {
    text: String,
    direction: Direction,
    changed: bool,
}

struct SessionEngine {
    tracker: AutoWordTracker,
    policy: AutoCorrectPolicy,
    mode: SessionMode,
    source_layout: Option<InputLayout>,
    mapping: Option<Vec<(char, char)>>,
}

impl SessionEngine {
    fn new(options: SessionOptions) -> Result<Self, String> {
        if !(1..=64).contains(&options.min_word_length) {
            return Err("minWordLength must be between 1 and 64".to_owned());
        }
        if options.exceptions.len() > 256 {
            return Err("exceptions cannot contain more than 256 entries".to_owned());
        }
        if options
            .exceptions
            .iter()
            .any(|exception| exception.chars().count() > 128)
        {
            return Err("each exception must contain at most 128 characters".to_owned());
        }
        if options.mapping_overrides.len() > 128 {
            return Err("mappingOverrides cannot contain more than 128 entries".to_owned());
        }

        let mapping = mapping_with_overrides(&options.mapping_overrides)?;
        let mut tracker = AutoWordTracker::default();
        tracker.set_source_layout(options.source_layout);
        Ok(Self {
            tracker,
            policy: AutoCorrectPolicy {
                sensitivity: options.sensitivity,
                min_word_length: options.min_word_length,
                exceptions: options.exceptions,
                triggers: upyr_core::builtin_triggers(),
            },
            mode: options.mode,
            source_layout: options.source_layout,
            mapping,
        })
    }

    fn configure(&mut self, options: SessionOptions) -> Result<(), String> {
        *self = Self::new(options)?;
        Ok(())
    }

    fn set_source_layout(&mut self, layout: Option<InputLayout>) {
        self.source_layout = layout;
        self.tracker.set_source_layout(layout);
    }

    fn reset(&mut self) {
        self.tracker.clear();
        self.tracker.set_source_layout(self.source_layout);
    }

    fn convert_text(&self, input: &str, direction: Direction) -> ConversionResult {
        let conversion = match self.mapping.as_deref() {
            Some(mapping) => convert_with_mapping(input, direction, mapping),
            None => convert(input, direction),
        };
        ConversionResult {
            text: conversion.text,
            direction: conversion.direction,
            changed: conversion.changed,
        }
    }

    fn key_down(&mut self, input: KeyDownInput) -> WebDecision {
        if input.is_composing || is_composition_key(&input.key) {
            self.reset();
            return WebDecision::passive(DecisionKind::Reset, "composition", self.source_layout);
        }

        let key = physical_key_from_code(&input.code);
        if key == PhysicalKey::Unsupported {
            self.reset();
            return WebDecision::passive(
                DecisionKind::Reset,
                "unsupported-code",
                self.source_layout,
            );
        }

        if input.alt_graph_key {
            self.reset();
            return WebDecision::passive(DecisionKind::Reset, "alt-graph", self.source_layout);
        }

        if (input.ctrl_key || input.alt_key || input.meta_key) && !is_modifier_key(key) {
            self.reset();
            return WebDecision::passive(DecisionKind::Reset, "modified-input", self.source_layout);
        }

        // The core currently carries one case bit. These keys are punctuation
        // in English but letters in Ukrainian, so Caps Lock would require two
        // independently rendered case states. Abstain instead of producing a
        // replacement that cannot exactly match both sides.
        if input.caps_lock && caps_lock_is_layout_asymmetric(key, self.mapping.as_deref()) {
            self.reset();
            return WebDecision::passive(
                DecisionKind::Reset,
                "caps-lock-layout-asymmetry",
                self.source_layout,
            );
        }

        if self.source_layout.is_none() && AutoWordTracker::can_begin(key) {
            self.reset();
            return WebDecision::passive(
                DecisionKind::Reset,
                "source-layout-required",
                self.source_layout,
            );
        }

        // AutoWordTracker::clear intentionally discards its layout. The web
        // session owns the durable layout and reapplies it before every event.
        self.tracker.set_source_layout(self.source_layout);
        let event = PhysicalKeyEvent {
            key,
            shifted: effective_shift(key, input.shift_key, input.caps_lock),
        };
        let sample = self.tracker.observe(event);
        let Some(sample) = sample else {
            if key == PhysicalKey::Space {
                // Leading/repeated whitespace is not represented in the core
                // context; keeping the older context would make suffix
                // verification fail on a later correction.
                self.reset();
                return WebDecision::passive(
                    DecisionKind::Reset,
                    "empty-boundary",
                    self.source_layout,
                );
            }
            return WebDecision::passive(DecisionKind::Wait, "tracking", self.source_layout);
        };

        match evaluate(&sample, &self.policy, self.mapping.as_deref()) {
            AutoDecision::Correct(correction) => {
                let kind = match self.mode {
                    SessionMode::Suggest => DecisionKind::Suggest,
                    SessionMode::Auto => DecisionKind::Correct,
                };
                self.reset();
                WebDecision::correction(kind, self.source_layout, correction)
            }
            AutoDecision::Continue => {
                WebDecision::passive(DecisionKind::Wait, "ambiguous", self.source_layout)
            }
            AutoDecision::Reset => {
                self.reset();
                WebDecision::passive(DecisionKind::Reset, "keep-source", self.source_layout)
            }
        }
    }
}

fn mapping_with_overrides(
    overrides: &[MappingOverride],
) -> Result<Option<Vec<(char, char)>>, String> {
    if overrides.is_empty() {
        return Ok(None);
    }

    let mut replacements = Vec::with_capacity(overrides.len() * 2);
    let mut overridden_english = std::collections::HashSet::new();
    for mapping_override in overrides {
        let expected_shifted_english = shifted_english_for(mapping_override.english).ok_or_else(|| {
            format!(
                "mappingOverrides English endpoint `{}` is not a built-in unshifted physical key",
                mapping_override.english
            )
        })?;
        let shifted = match (
            mapping_override.shifted_english,
            mapping_override.shifted_ukrainian,
        ) {
            (None, None) => None,
            (Some(english), Some(ukrainian)) if english == expected_shifted_english => {
                Some((english, ukrainian))
            }
            (Some(english), Some(_)) => {
                return Err(format!(
                    "shiftedEnglish `{english}` is not the Shift layer of `{}`; expected `{expected_shifted_english}`",
                    mapping_override.english
                ));
            }
            _ => {
                return Err(
                    "shiftedEnglish and shiftedUkrainian must be provided together".to_owned(),
                );
            }
        };
        for pair in
            std::iter::once((mapping_override.english, mapping_override.ukrainian)).chain(shifted)
        {
            if !overridden_english.insert(pair.0) {
                return Err(format!(
                    "mappingOverrides contains duplicate English endpoint `{}`",
                    pair.0
                ));
            }
            replacements.push(pair);
        }
    }

    let mut mapping = default_physical_mapping();
    for (override_english, override_ukrainian) in replacements {
        if let Some((_, ukrainian)) = mapping
            .iter_mut()
            .find(|(english, _)| *english == override_english)
        {
            *ukrainian = override_ukrainian;
        } else {
            mapping.push((override_english, override_ukrainian));
        }
    }

    let mut target_sources = std::collections::HashMap::<char, Vec<char>>::new();
    for (english, ukrainian) in &mapping {
        target_sources.entry(*ukrainian).or_default().push(*english);
    }
    for (ukrainian, english_sources) in target_sources {
        if english_sources.len() <= 1 {
            continue;
        }
        let intentional_shift_alias = english_sources.len() == 2
            && overrides.iter().any(|mapping_override| {
                mapping_override.ukrainian == ukrainian
                    && mapping_override.shifted_ukrainian == Some(ukrainian)
                    && mapping_override.shifted_english.is_some_and(|shifted| {
                        english_sources.contains(&mapping_override.english)
                            && english_sources.contains(&shifted)
                    })
            });
        if !intentional_shift_alias {
            return Err(format!(
                "mappingOverrides creates an ambiguous Ukrainian endpoint `{ukrainian}`"
            ));
        }
    }

    Ok(Some(mapping))
}

fn shifted_english_for(unshifted: char) -> Option<char> {
    if unshifted.is_ascii_lowercase() {
        return Some(unshifted.to_ascii_uppercase());
    }
    match unshifted {
        '`' => Some('~'),
        '[' => Some('{'),
        ']' => Some('}'),
        '\\' => Some('|'),
        ';' => Some(':'),
        '\'' => Some('"'),
        ',' => Some('<'),
        '.' => Some('>'),
        '/' => Some('?'),
        _ => None,
    }
}

fn physical_key_from_code(code: &str) -> PhysicalKey {
    if let Some(letter) = code.strip_prefix("Key") {
        if letter.len() == 1 && letter.as_bytes()[0].is_ascii_uppercase() {
            return PhysicalKey::from_ascii_letter(char::from(letter.as_bytes()[0]))
                .map_or(PhysicalKey::Unsupported, |(key, _)| key);
        }
    }

    match code {
        "Backquote" => PhysicalKey::Backquote,
        "BracketLeft" => PhysicalKey::BracketLeft,
        "BracketRight" => PhysicalKey::BracketRight,
        "Backslash" => PhysicalKey::Backslash,
        "Semicolon" => PhysicalKey::Semicolon,
        "Quote" => PhysicalKey::Quote,
        "Comma" => PhysicalKey::Comma,
        "Period" => PhysicalKey::Period,
        "Slash" => PhysicalKey::Slash,
        "Space" => PhysicalKey::Space,
        "Backspace" => PhysicalKey::Backspace,
        "ShiftLeft" | "ShiftRight" => PhysicalKey::Shift,
        "CapsLock" => PhysicalKey::CapsLock,
        "ControlLeft" | "ControlRight" => PhysicalKey::Control,
        "AltLeft" | "AltRight" => PhysicalKey::Alt,
        "MetaLeft" | "MetaRight" | "OSLeft" | "OSRight" => PhysicalKey::Meta,
        _ => PhysicalKey::Unsupported,
    }
}

fn effective_shift(key: PhysicalKey, shift_key: bool, caps_lock: bool) -> bool {
    if is_letter_key(key) {
        shift_key ^ caps_lock
    } else {
        shift_key
    }
}

fn is_letter_key(key: PhysicalKey) -> bool {
    matches!(
        key,
        PhysicalKey::KeyA
            | PhysicalKey::KeyB
            | PhysicalKey::KeyC
            | PhysicalKey::KeyD
            | PhysicalKey::KeyE
            | PhysicalKey::KeyF
            | PhysicalKey::KeyG
            | PhysicalKey::KeyH
            | PhysicalKey::KeyI
            | PhysicalKey::KeyJ
            | PhysicalKey::KeyK
            | PhysicalKey::KeyL
            | PhysicalKey::KeyM
            | PhysicalKey::KeyN
            | PhysicalKey::KeyO
            | PhysicalKey::KeyP
            | PhysicalKey::KeyQ
            | PhysicalKey::KeyR
            | PhysicalKey::KeyS
            | PhysicalKey::KeyT
            | PhysicalKey::KeyU
            | PhysicalKey::KeyV
            | PhysicalKey::KeyW
            | PhysicalKey::KeyX
            | PhysicalKey::KeyY
            | PhysicalKey::KeyZ
    )
}

fn is_modifier_key(key: PhysicalKey) -> bool {
    matches!(
        key,
        PhysicalKey::Shift
            | PhysicalKey::CapsLock
            | PhysicalKey::Control
            | PhysicalKey::Alt
            | PhysicalKey::Meta
    )
}

fn is_composition_key(key: &str) -> bool {
    matches!(key, "Dead" | "Process" | "Unidentified")
}

fn caps_lock_is_layout_asymmetric(key: PhysicalKey, mapping: Option<&[(char, char)]>) -> bool {
    let Some(english) = unshifted_punctuation(key) else {
        return false;
    };
    match mapping {
        Some(mapping) => mapping
            .iter()
            .find_map(|(candidate, target)| (*candidate == english).then_some(*target))
            .is_some_and(char::is_alphabetic),
        None => matches!(
            key,
            PhysicalKey::BracketLeft
                | PhysicalKey::BracketRight
                | PhysicalKey::Backslash
                | PhysicalKey::Semicolon
                | PhysicalKey::Quote
                | PhysicalKey::Comma
                | PhysicalKey::Period
        ),
    }
}

fn unshifted_punctuation(key: PhysicalKey) -> Option<char> {
    match key {
        PhysicalKey::Backquote => Some('`'),
        PhysicalKey::BracketLeft => Some('['),
        PhysicalKey::BracketRight => Some(']'),
        PhysicalKey::Backslash => Some('\\'),
        PhysicalKey::Semicolon => Some(';'),
        PhysicalKey::Quote => Some('\''),
        PhysicalKey::Comma => Some(','),
        PhysicalKey::Period => Some('.'),
        PhysicalKey::Slash => Some('/'),
        _ => None,
    }
}

fn parse_direction(value: &str) -> Result<Direction, String> {
    match value {
        "smart" => Ok(Direction::Smart),
        "english-to-ukrainian" => Ok(Direction::EnglishToUkrainian),
        "ukrainian-to-english" => Ok(Direction::UkrainianToEnglish),
        _ => Err(format!(
            "unsupported direction `{value}`; expected smart, english-to-ukrainian, or ukrainian-to-english"
        )),
    }
}

fn parse_layout(value: &str) -> Result<InputLayout, String> {
    match value {
        "english" | "en" => Ok(InputLayout::English),
        "ukrainian" | "uk" | "uk-UA" => Ok(InputLayout::Ukrainian),
        _ => Err(format!(
            "unsupported source layout `{value}`; expected english or ukrainian"
        )),
    }
}

fn decode_options(value: Option<JsValue>) -> Result<SessionOptions, JsValue> {
    match value {
        None => Ok(SessionOptions::default()),
        Some(value) if value.is_null() || value.is_undefined() => Ok(SessionOptions::default()),
        Some(value) => {
            validate_option_keys(&value)?;
            serde_wasm_bindgen::from_value(value)
                .map_err(|error| JsValue::from_str(&format!("invalid Upyr options: {error}")))
        }
    }
}

fn validate_option_keys(value: &JsValue) -> Result<(), JsValue> {
    validate_object_keys(
        value,
        &[
            "mode",
            "sensitivity",
            "minWordLength",
            "exceptions",
            "sourceLayout",
            "mappingOverrides",
        ],
        "Upyr options",
    )?;

    let mappings = js_sys::Reflect::get(value, &JsValue::from_str("mappingOverrides"))
        .map_err(|_| JsValue::from_str("could not read mappingOverrides"))?;
    if mappings.is_null() || mappings.is_undefined() {
        return Ok(());
    }
    if !js_sys::Array::is_array(&mappings) {
        return Ok(());
    }
    for entry in js_sys::Array::from(&mappings).iter() {
        validate_object_keys(
            &entry,
            &["english", "ukrainian", "shiftedEnglish", "shiftedUkrainian"],
            "mappingOverrides entry",
        )?;
    }
    Ok(())
}

fn validate_object_keys(value: &JsValue, allowed: &[&str], label: &str) -> Result<(), JsValue> {
    if !value.is_object() || js_sys::Array::is_array(value) {
        return Ok(());
    }
    let object = js_sys::Object::from(value.clone());
    for key in js_sys::Object::keys(&object).iter() {
        let Some(key) = key.as_string() else {
            continue;
        };
        if !allowed.contains(&key.as_str()) {
            return Err(JsValue::from_str(&format!("unknown {label} field `{key}`")));
        }
    }
    Ok(())
}

fn to_js_value<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    serde_wasm_bindgen::to_value(value)
        .map_err(|error| JsValue::from_str(&format!("could not serialize Upyr result: {error}")))
}

fn default_conversion(input: &str, direction: Direction) -> ConversionResult {
    let conversion = convert(input, direction);
    ConversionResult {
        text: conversion.text,
        direction: conversion.direction,
        changed: conversion.changed,
    }
}

/// Converts a string with Upyr's built-in physical-key mapping.
#[wasm_bindgen(
    js_name = convertText,
    unchecked_return_type = "UpyrConversionResult"
)]
pub fn convert_text(
    input: &str,
    #[wasm_bindgen(unchecked_param_type = "UpyrRequestedDirection")] direction: &str,
) -> Result<JsValue, JsValue> {
    let direction = parse_direction(direction).map_err(|error| JsValue::from_str(&error))?;
    to_js_value(&default_conversion(input, direction))
}

/// Returns the identifier of the embedded correction model.
#[wasm_bindgen(js_name = modelVersion)]
pub fn model_version() -> String {
    MODEL_VERSION.to_owned()
}

/// Stateful, DOM-independent browser correction session.
#[wasm_bindgen]
pub struct UpyrSession {
    engine: SessionEngine,
}

#[wasm_bindgen]
impl UpyrSession {
    /// Creates a session. Missing or `undefined` options use conservative,
    /// suggestion-only defaults.
    #[wasm_bindgen(constructor)]
    pub fn new(options: Option<JsSessionOptions>) -> Result<Self, JsValue> {
        let options = decode_options(options.map(JsValue::from))?;
        let engine = SessionEngine::new(options).map_err(|error| JsValue::from_str(&error))?;
        Ok(Self { engine })
    }

    /// Replaces all session options and resets tracked input.
    pub fn configure(&mut self, options: Option<JsSessionOptions>) -> Result<(), JsValue> {
        let options = decode_options(options.map(JsValue::from))?;
        self.engine
            .configure(options)
            .map_err(|error| JsValue::from_str(&error))
    }

    /// Sets or clears the layout that produced the observed browser input.
    #[wasm_bindgen(js_name = setSourceLayout)]
    pub fn set_source_layout(&mut self, layout: Option<JsLayout>) -> Result<(), JsValue> {
        let layout = layout
            .map(JsValue::from)
            .map(|value| {
                value
                    .as_string()
                    .ok_or_else(|| JsValue::from_str("source layout must be a string"))
            })
            .transpose()?
            .as_deref()
            .map(parse_layout)
            .transpose()
            .map_err(|error| JsValue::from_str(&error))?;
        self.engine.set_source_layout(layout);
        Ok(())
    }

    /// Processes normalized data from one browser `keydown` event.
    #[wasm_bindgen(js_name = keyDown, unchecked_return_type = "UpyrDecision")]
    pub fn key_down(&mut self, input: JsKeyDownInput) -> Result<JsValue, JsValue> {
        let input: KeyDownInput = serde_wasm_bindgen::from_value(input.into())
            .map_err(|error| JsValue::from_str(&format!("invalid keyDown input: {error}")))?;
        to_js_value(&self.engine.key_down(input))
    }

    /// Clears tracked text without changing the configured layout or policy.
    pub fn reset(&mut self) {
        self.engine.reset();
    }

    /// Converts text with this session's physical mapping overrides.
    #[wasm_bindgen(
        js_name = convertText,
        unchecked_return_type = "UpyrConversionResult"
    )]
    pub fn convert_text(
        &self,
        input: &str,
        #[wasm_bindgen(unchecked_param_type = "UpyrRequestedDirection")] direction: &str,
    ) -> Result<JsValue, JsValue> {
        let direction = parse_direction(direction).map_err(|error| JsValue::from_str(&error))?;
        to_js_value(&self.engine.convert_text(input, direction))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn options(layout: Option<InputLayout>, mode: SessionMode) -> SessionOptions {
        SessionOptions {
            source_layout: layout,
            mode,
            ..SessionOptions::default()
        }
    }

    fn key(code: &str) -> KeyDownInput {
        KeyDownInput {
            code: code.to_owned(),
            key: code.to_owned(),
            shift_key: false,
            caps_lock: false,
            ctrl_key: false,
            alt_key: false,
            meta_key: false,
            alt_graph_key: false,
            is_composing: false,
        }
    }

    fn type_codes(engine: &mut SessionEngine, codes: &[&str]) -> WebDecision {
        let mut decision = WebDecision::passive(DecisionKind::Wait, "test", None);
        for code in codes {
            decision = engine.key_down(key(code));
        }
        decision
    }

    #[test]
    fn converts_text_explicitly_in_both_directions() {
        let english_to_ukrainian = default_conversion("ghbdsn", Direction::EnglishToUkrainian);
        let ukrainian_to_english = default_conversion("руддщ", Direction::UkrainianToEnglish);

        assert_eq!(english_to_ukrainian.text, "привіт");
        assert_eq!(ukrainian_to_english.text, "hello");
    }

    #[test]
    fn maps_browser_codes_and_applies_caps_lock_only_to_letters() {
        assert_eq!(physical_key_from_code("KeyG"), PhysicalKey::KeyG);
        assert_eq!(
            physical_key_from_code("BracketLeft"),
            PhysicalKey::BracketLeft
        );
        assert_eq!(
            physical_key_from_code("IntlBackslash"),
            PhysicalKey::Unsupported
        );
        assert!(effective_shift(PhysicalKey::KeyG, false, true));
        assert!(!effective_shift(PhysicalKey::KeyG, true, true));
        assert!(!effective_shift(PhysicalKey::Comma, false, true));
        assert!(effective_shift(PhysicalKey::Comma, true, true));
    }

    #[test]
    fn suggests_a_wrong_layout_word_after_the_boundary_input() {
        let mut engine =
            SessionEngine::new(options(Some(InputLayout::English), SessionMode::Suggest)).unwrap();

        let decision = type_codes(
            &mut engine,
            &["KeyG", "KeyH", "KeyB", "KeyD", "KeyS", "KeyN", "Space"],
        );

        assert_eq!(decision.kind, DecisionKind::Suggest);
        assert_eq!(decision.expected_source.as_deref(), Some("ghbdsn "));
        assert_eq!(decision.replacement.as_deref(), Some("привіт "));
        assert_eq!(decision.direction, Some(Direction::EnglishToUkrainian));
        assert_eq!(decision.target_layout, Some(InputLayout::Ukrainian));
        assert!(decision.apply_after_input);
    }

    #[test]
    fn auto_mode_returns_a_correction_action() {
        let mut engine =
            SessionEngine::new(options(Some(InputLayout::English), SessionMode::Auto)).unwrap();

        let decision = type_codes(
            &mut engine,
            &["KeyG", "KeyH", "KeyB", "KeyD", "KeyS", "KeyN", "Space"],
        );

        assert_eq!(decision.kind, DecisionKind::Correct);
    }

    #[test]
    fn recognizes_the_reverse_direction_from_physical_codes() {
        let mut engine =
            SessionEngine::new(options(Some(InputLayout::Ukrainian), SessionMode::Suggest))
                .unwrap();

        let decision = type_codes(
            &mut engine,
            &["KeyH", "KeyE", "KeyL", "KeyL", "KeyO", "Space"],
        );

        assert_eq!(decision.kind, DecisionKind::Suggest);
        assert_eq!(decision.expected_source.as_deref(), Some("руддщ "));
        assert_eq!(decision.replacement.as_deref(), Some("hello "));
        assert_eq!(decision.direction, Some(Direction::UkrainianToEnglish));
    }

    #[test]
    fn preserves_physical_punctuation_that_forms_a_ukrainian_word() {
        let mut engine =
            SessionEngine::new(options(Some(InputLayout::English), SessionMode::Suggest)).unwrap();

        let decision = type_codes(
            &mut engine,
            &["BracketLeft", "KeyK", "KeyS", "Comma", "Space"],
        );

        assert_eq!(decision.kind, DecisionKind::Suggest);
        assert_eq!(decision.expected_source.as_deref(), Some("[ks, "));
        assert_eq!(decision.replacement.as_deref(), Some("хліб "));
    }

    #[test]
    fn backspace_keeps_the_physical_sequence_synchronized() {
        let mut engine =
            SessionEngine::new(options(Some(InputLayout::English), SessionMode::Suggest)).unwrap();

        let decision = type_codes(
            &mut engine,
            &[
                "KeyG",
                "KeyH",
                "KeyB",
                "KeyD",
                "KeyS",
                "KeyX",
                "Backspace",
                "KeyN",
                "Space",
            ],
        );

        assert_eq!(decision.kind, DecisionKind::Suggest);
        assert_eq!(decision.expected_source.as_deref(), Some("ghbdsn "));
        assert_eq!(decision.replacement.as_deref(), Some("привіт "));
    }

    #[test]
    fn changing_layout_discards_a_partial_word() {
        let mut engine =
            SessionEngine::new(options(Some(InputLayout::English), SessionMode::Suggest)).unwrap();
        type_codes(&mut engine, &["KeyG", "KeyH"]);
        engine.set_source_layout(Some(InputLayout::Ukrainian));

        let decision = type_codes(
            &mut engine,
            &["KeyH", "KeyE", "KeyL", "KeyL", "KeyO", "Space"],
        );

        assert_eq!(decision.kind, DecisionKind::Suggest);
        assert_eq!(decision.expected_source.as_deref(), Some("руддщ "));
        assert_eq!(decision.replacement.as_deref(), Some("hello "));
    }

    #[test]
    fn keeps_native_text_and_reuses_one_layout_setting_for_the_next_word() {
        let mut engine =
            SessionEngine::new(options(Some(InputLayout::English), SessionMode::Suggest)).unwrap();

        let native = type_codes(
            &mut engine,
            &["KeyH", "KeyE", "KeyL", "KeyL", "KeyO", "Space"],
        );
        let wrong_layout = type_codes(
            &mut engine,
            &["KeyG", "KeyH", "KeyB", "KeyD", "KeyS", "KeyN", "Space"],
        );

        assert_eq!(native.kind, DecisionKind::Reset);
        assert_eq!(native.reason, "keep-source");
        assert_eq!(wrong_layout.kind, DecisionKind::Suggest);
    }

    #[test]
    fn resets_for_composition_and_modified_input_without_poisoning_the_next_word() {
        let mut engine =
            SessionEngine::new(options(Some(InputLayout::English), SessionMode::Suggest)).unwrap();
        engine.key_down(key("KeyG"));
        let mut composing = key("KeyH");
        composing.is_composing = true;
        assert_eq!(engine.key_down(composing).reason, "composition");

        engine.key_down(key("KeyG"));
        let mut shortcut = key("KeyC");
        shortcut.meta_key = true;
        assert_eq!(engine.key_down(shortcut).reason, "modified-input");

        let decision = type_codes(
            &mut engine,
            &["KeyG", "KeyH", "KeyB", "KeyD", "KeyS", "KeyN", "Space"],
        );
        assert_eq!(decision.kind, DecisionKind::Suggest);
    }

    #[test]
    fn requires_a_source_layout_before_tracking_text() {
        let mut engine = SessionEngine::new(SessionOptions::default()).unwrap();

        let decision = engine.key_down(key("KeyG"));

        assert_eq!(decision.kind, DecisionKind::Reset);
        assert_eq!(decision.reason, "source-layout-required");
    }

    #[test]
    fn abstains_on_caps_lock_for_layout_asymmetric_punctuation_keys() {
        let mut engine =
            SessionEngine::new(options(Some(InputLayout::English), SessionMode::Suggest)).unwrap();
        let mut input = key("BracketLeft");
        input.caps_lock = true;

        let decision = engine.key_down(input);

        assert_eq!(decision.kind, DecisionKind::Reset);
        assert_eq!(decision.reason, "caps-lock-layout-asymmetry");
    }

    #[test]
    fn applies_physical_mapping_overrides_to_session_conversion() {
        let mut session_options = options(Some(InputLayout::English), SessionMode::Suggest);
        session_options.mapping_overrides = vec![MappingOverride {
            english: '\\',
            ukrainian: 'ʼ',
            shifted_english: Some('|'),
            shifted_ukrainian: Some('ʼ'),
        }];
        let engine = SessionEngine::new(session_options).unwrap();

        let result = engine.convert_text("[];'\\,./|", Direction::EnglishToUkrainian);
        let reverse = engine.convert_text("ʼ", Direction::UkrainianToEnglish);

        assert_eq!(result.text, "хїжєʼбю.ʼ");
        assert_eq!(reverse.text, "\\");
    }

    #[test]
    fn rejects_incomplete_or_ambiguous_mapping_overrides() {
        let incomplete_shift = vec![MappingOverride {
            english: '\\',
            ukrainian: 'ʼ',
            shifted_english: Some('|'),
            shifted_ukrainian: None,
        }];
        let duplicate_target = vec![MappingOverride {
            english: 'x',
            ukrainian: 'ф',
            shifted_english: None,
            shifted_ukrainian: None,
        }];
        let unrelated_shift = vec![MappingOverride {
            english: '\\',
            ukrainian: 'ʼ',
            shifted_english: Some('x'),
            shifted_ukrainian: Some('ʼ'),
        }];
        let swapped_layers = vec![MappingOverride {
            english: '|',
            ukrainian: 'ʼ',
            shifted_english: Some('\\'),
            shifted_ukrainian: Some('ʼ'),
        }];
        let unknown_key = vec![MappingOverride {
            english: '1',
            ukrainian: 'ґ',
            shifted_english: None,
            shifted_ukrainian: None,
        }];

        assert!(mapping_with_overrides(&incomplete_shift).is_err());
        assert!(mapping_with_overrides(&duplicate_target).is_err());
        assert!(mapping_with_overrides(&unrelated_shift).is_err());
        assert!(mapping_with_overrides(&swapped_layers).is_err());
        assert!(mapping_with_overrides(&unknown_key).is_err());
    }

    #[test]
    fn accepted_mapping_override_round_trips() {
        let mapping = mapping_with_overrides(&[MappingOverride {
            english: 'x',
            ukrainian: '§',
            shifted_english: None,
            shifted_ukrainian: None,
        }])
        .unwrap()
        .unwrap();

        let forward = convert_with_mapping("x", Direction::EnglishToUkrainian, &mapping);
        let reverse = convert_with_mapping("§", Direction::UkrainianToEnglish, &mapping);

        assert_eq!(forward.text, "§");
        assert_eq!(reverse.text, "x");
    }

    #[test]
    fn rejects_invalid_policy_bounds_and_direction_names() {
        let invalid = SessionOptions {
            min_word_length: 0,
            ..SessionOptions::default()
        };

        assert!(SessionEngine::new(invalid).is_err());
        assert!(parse_direction("sideways").is_err());
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_contract_tests {
    use super::*;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_test::wasm_bindgen_test;

    fn js<T: Serialize>(value: &T) -> JsValue {
        serde_wasm_bindgen::to_value(value).unwrap()
    }

    fn js_options<T: Serialize>(value: &T) -> JsSessionOptions {
        js(value).unchecked_into()
    }

    fn js_key<T: Serialize>(value: &T) -> JsKeyDownInput {
        js(value).unchecked_into()
    }

    fn normalized_key(code: &str) -> KeyDownInput {
        KeyDownInput {
            code: code.to_owned(),
            key: code.to_owned(),
            shift_key: false,
            caps_lock: false,
            ctrl_key: false,
            alt_key: false,
            meta_key: false,
            alt_graph_key: false,
            is_composing: false,
        }
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct DecisionOutput {
        kind: DecisionKind,
        reason: String,
        apply_after_input: bool,
        expected_source: Option<String>,
        replacement: Option<String>,
    }

    #[wasm_bindgen_test]
    fn camel_case_session_contract_round_trips_through_javascript_values() {
        let options = SessionOptions {
            source_layout: Some(InputLayout::English),
            mapping_overrides: vec![MappingOverride {
                english: '\\',
                ukrainian: 'ʼ',
                shifted_english: Some('|'),
                shifted_ukrainian: Some('ʼ'),
            }],
            ..SessionOptions::default()
        };
        let mut session = UpyrSession::new(Some(js_options(&options))).unwrap();

        let mut decision = None;
        for code in ["KeyG", "KeyH", "KeyB", "KeyD", "KeyS", "KeyN", "Space"] {
            let input = normalized_key(code);
            decision = Some(session.key_down(js_key(&input)).unwrap());
        }
        let decision: DecisionOutput =
            serde_wasm_bindgen::from_value(decision.expect("last decision")).unwrap();
        let mapped: ConversionResult = serde_wasm_bindgen::from_value(
            session
                .convert_text("[];'\\,./", "english-to-ukrainian")
                .unwrap(),
        )
        .unwrap();

        assert_eq!(decision.kind, DecisionKind::Suggest);
        assert_eq!(decision.expected_source.as_deref(), Some("ghbdsn "));
        assert_eq!(decision.replacement.as_deref(), Some("привіт "));
        assert!(decision.apply_after_input);
        assert_eq!(mapped.text, "хїжєʼбю.");
    }

    #[wasm_bindgen_test]
    fn undefined_options_and_explicit_conversion_use_the_exported_abi() {
        let mut session = UpyrSession::new(Some(JsValue::UNDEFINED.unchecked_into())).unwrap();
        let no_layout: DecisionOutput = serde_wasm_bindgen::from_value(
            session.key_down(js_key(&normalized_key("KeyG"))).unwrap(),
        )
        .unwrap();
        let conversion: ConversionResult = serde_wasm_bindgen::from_value(
            super::convert_text("ghbdsn", "english-to-ukrainian").unwrap(),
        )
        .unwrap();

        assert_eq!(no_layout.reason, "source-layout-required");
        assert_eq!(conversion.text, "привіт");
        assert_eq!(model_version(), MODEL_VERSION);

        session
            .set_source_layout(Some(JsValue::from_str("english").unchecked_into()))
            .unwrap();
        let tracking: DecisionOutput = serde_wasm_bindgen::from_value(
            session.key_down(js_key(&normalized_key("KeyG"))).unwrap(),
        )
        .unwrap();
        assert_eq!(tracking.kind, DecisionKind::Wait);
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct MisspelledOptions {
        min_word_lenght: usize,
    }

    #[wasm_bindgen_test]
    fn rejects_unknown_option_names() {
        let result = UpyrSession::new(Some(js_options(&MisspelledOptions { min_word_lenght: 4 })));

        assert!(result.is_err());
    }

    #[derive(Serialize)]
    struct IncompleteKeyDown<'a> {
        code: &'a str,
    }

    #[wasm_bindgen_test]
    fn rejects_incomplete_keydown_metadata() {
        let mut session = UpyrSession::new(None).unwrap();
        let result = session.key_down(js_key(&IncompleteKeyDown { code: "KeyG" }));

        assert!(result.is_err());
    }
}

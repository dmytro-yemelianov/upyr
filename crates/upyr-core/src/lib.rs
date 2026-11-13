#![deny(unsafe_code)]

pub mod auto_correct;
pub mod layout;
pub mod transform;
pub mod triggers;

pub use auto_correct::{
    AutoCorrectPolicy, AutoCorrection, AutoDecision, AutoWordTracker, InputLayout,
    PartialWordState, PhysicalKey, PhysicalKeyEvent, Sensitivity, WordSample,
    contextual_edit_is_bounded, evaluate, evaluate_context, evaluate_partial, has_pending_context,
    physical_character,
};
pub use layout::{
    Conversion, Direction, convert, convert_with_mapping, default_physical_mapping,
    resolve_direction,
};
pub use transform::{TextTransform, cycle_case, transliterate};
pub use triggers::{Trigger, TriggerAction, builtin_triggers, parse_trigger, parse_triggers};

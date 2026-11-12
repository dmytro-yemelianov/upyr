//! Deterministic compiler used by build.rs and the table-equivalence tests.

use super::punto_model::Rule;
use super::punto_trie::{Edge, Node, ROOT_SLOTS, Trie, root_slot};
use std::borrow::Cow;

pub(super) const TRIGGERS: &str = include_str!("../../assets/punto/triggers.txt");
pub(super) const RULES: &str = include_str!("../../assets/punto/ps.txt");

impl Rule {
    pub(super) const fn new(
        rank: u32,
        mode: Mode,
        exception: bool,
        priority: bool,
        after_dash: bool,
    ) -> Self {
        assert!(
            rank <= u32::MAX >> 5,
            "Punto rule rank exceeds packed representation"
        );
        let mode = match mode {
            Mode::Anywhere => 0,
            Mode::Exact => 1,
            Mode::Beginning => 2,
        };
        Self(
            (rank << 5)
                | mode
                | ((exception as u32) << 2)
                | ((priority as u32) << 3)
                | ((after_dash as u32) << 4),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Anywhere,
    Exact,
    Beginning,
}

const NONE: u32 = u32::MAX;

struct BuildNode {
    edges: Vec<(char, u32)>,
    first_value: u32,
    last_value: u32,
}

impl BuildNode {
    fn new() -> Self {
        Self {
            edges: Vec::new(),
            first_value: NONE,
            last_value: NONE,
        }
    }
}

pub(super) struct Builder<T: Copy + 'static> {
    nodes: Vec<BuildNode>,
    // One arena replaces the thousands of small terminal allocations. Links
    // retain insertion order until finish packs each terminal contiguously.
    values: Vec<(T, u32)>,
}

impl<T: Copy + 'static> Builder<T> {
    pub(super) fn new() -> Self {
        Self {
            nodes: vec![BuildNode::new()],
            values: Vec::new(),
        }
    }

    pub(super) fn insert(&mut self, text: &str, value: T) {
        let mut node = 0usize;
        for character in text.chars() {
            node = match self.nodes[node]
                .edges
                .binary_search_by_key(&character, |e| e.0)
            {
                Ok(index) => self.nodes[node].edges[index].1 as usize,
                Err(index) => {
                    let next = self.nodes.len();
                    let encoded = u32::try_from(next).expect("Punto trie fits u32");
                    self.nodes.push(BuildNode::new());
                    self.nodes[node].edges.insert(index, (character, encoded));
                    next
                }
            };
        }
        let index = u32::try_from(self.values.len()).expect("Punto values fit u32");
        self.values.push((value, NONE));
        let node = &mut self.nodes[node];
        if node.last_value == NONE {
            node.first_value = index;
        } else {
            self.values[node.last_value as usize].1 = index;
        }
        node.last_value = index;
    }

    pub(super) fn finish(self) -> Trie<T> {
        // Node ID 65535 is reserved for a missing root edge. Fail at build
        // time if a future data import exceeds the compact format.
        assert!(
            self.nodes.len() <= u16::MAX as usize,
            "Punto trie exceeds u16 node IDs"
        );
        assert!(
            self.values.len() <= u16::MAX as usize,
            "Punto trie exceeds u16 value offsets"
        );
        let mut alphabet: Vec<char> = self
            .nodes
            .iter()
            .flat_map(|n| n.edges.iter().map(|e| e.0))
            .filter(|&c| c as u32 > u16::MAX as u32)
            .collect();
        alphabet.sort_unstable();
        alphabet.dedup();
        assert!(
            alphabet.len() <= 0x800,
            "Punto supplementary alphabet exceeds surrogate slots"
        );
        let mut nodes = Vec::with_capacity(self.nodes.len());
        let mut edges = Vec::with_capacity(self.nodes.iter().map(|n| n.edges.len()).sum());
        let mut values = Vec::with_capacity(self.values.len());
        for node in self.nodes {
            let value_start = values.len();
            let mut index = node.first_value;
            while index != NONE {
                let (value, next) = self.values[index as usize];
                values.push(value);
                index = next;
            }
            nodes.push(Node {
                edge_start: u16::try_from(edges.len()).expect("Punto edges fit u16"),
                value_start: u16::try_from(value_start).expect("Punto values fit u16"),
                edge_count: u16::try_from(node.edges.len()).expect("Punto alphabet fits u16"),
                value_count: u16::try_from(values.len() - value_start)
                    .expect("Punto terminal fits u16"),
            });
            edges.extend(node.edges.into_iter().map(|(character, next)| Edge {
                symbol: if (character as u32) <= u16::MAX as u32 {
                    character as u16
                } else {
                    0xd800 + alphabet.binary_search(&character).unwrap() as u16
                },
                next: u16::try_from(next).expect("Punto node ID fits u16"),
            }));
            // Unicode order differs from symbol-code order for uncommon
            // characters, so sort on the encoded key used by runtime lookup.
            let start = nodes.last().unwrap().edge_start as usize;
            edges[start..].sort_unstable_by_key(|e| e.symbol);
        }
        // Every substring starts here. A small direct index for ASCII and
        // Cyrillic avoids repeatedly searching the widest branch of the trie.
        let mut root = vec![u16::MAX; ROOT_SLOTS];
        for edge in &edges[..nodes[0].edge_count as usize] {
            if let Some(slot) = char::from_u32(edge.symbol as u32).and_then(root_slot) {
                root[slot] = edge.next;
            }
        }
        Trie {
            nodes: Cow::Owned(nodes),
            edges: Cow::Owned(edges),
            values: Cow::Owned(values),
            root: Cow::Owned(root),
            alphabet: Cow::Owned(alphabet),
        }
    }
}

pub(super) fn compile_rules(table: &str) -> Result<(Trie<Rule>, Trie<Rule>), &'static str> {
    let mut original = Builder::new();
    let mut lowercased = Builder::new();
    // The Windows loader ignores zero-length source rows before it builds
    // its ordered rule vector.  The retained table keeps that raw row for
    // byte-for-byte provenance, so remove it only for matcher semantics.
    let entries = table
        .lines()
        .skip(1)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return Err("missing Punto rules");
    }
    let count = entries.len();
    for (line_index, line) in entries.into_iter().enumerate() {
        let (tags, key) = match line.split_once(' ') {
            Some((tags, key)) if tags.starts_with('_') => (tags, key),
            _ => ("", line),
        };
        let priority = tags.contains('A');
        let exception = tags.contains('E');
        let group = if priority {
            0
        } else if exception {
            1
        } else {
            2
        };
        let rule = Rule::new(
            u32::try_from(group * count + line_index).expect("Punto rank fits u32"),
            if tags.contains('P') {
                Mode::Exact
            } else if tags.contains('B') {
                Mode::Beginning
            } else {
                Mode::Anywhere
            },
            exception,
            priority,
            tags.contains('D'),
        );
        // The native lowercased-input path compares against the raw key.
        // Do not lowercase `key`: upper-case keys without C intentionally
        // fail on that path.
        if tags.contains('C') {
            original.insert(key, rule);
        } else {
            lowercased.insert(key, rule);
        }
    }
    Ok((original.finish(), lowercased.finish()))
}

pub(super) fn compile_early() -> (Trie<()>, Trie<()>, Trie<()>) {
    let mut triggers = Builder::new();
    let mut exceptions = Builder::new();
    let mut case_sensitive_exceptions = Builder::new();
    // No alphabet, length, language, frequency, or hand-picked filtering.
    for line in TRIGGERS.lines().filter(|line| !line.is_empty()) {
        triggers.insert(&line.to_lowercase(), ());
    }
    for (tags, text) in exception_rules() {
        if tags.contains('C') {
            case_sensitive_exceptions.insert(text, ());
        } else {
            exceptions.insert(&text.to_lowercase(), ());
        }
    }
    (
        triggers.finish(),
        exceptions.finish(),
        case_sensitive_exceptions.finish(),
    )
}

/// Failure links and the best unanchored output on each suffix chain. Anchored
/// and exact outputs stay in the original trie and are never inherited.
pub(super) fn compile_failure(trie: &Trie<Rule>) -> (Vec<u16>, Vec<u32>) {
    let mut failure = vec![0; trie.nodes.len()];
    let mut anywhere = vec![u32::MAX; trie.nodes.len()];
    let mut queue = std::collections::VecDeque::new();
    let root = &trie.nodes[0];
    for edge in &trie.edges[..root.edge_count as usize] {
        queue.push_back(edge.next);
    }
    while let Some(node) = queue.pop_front() {
        anywhere[node as usize] = trie
            .values(node)
            .iter()
            .filter(|r| !r.anchored())
            .map(|r| r.0)
            .min()
            .unwrap_or(u32::MAX)
            .min(anywhere[failure[node as usize] as usize]);
        let entry = &trie.nodes[node as usize];
        let start = entry.edge_start as usize;
        for edge in &trie.edges[start..start + entry.edge_count as usize] {
            let c = if (0xd800..0xe000).contains(&edge.symbol) {
                trie.alphabet[(edge.symbol - 0xd800) as usize]
            } else {
                char::from_u32(u32::from(edge.symbol)).unwrap()
            };
            let mut fallback = failure[node as usize];
            loop {
                if let Some(next) = trie.next(fallback, c) {
                    failure[edge.next as usize] = next;
                    break;
                }
                if fallback == 0 {
                    break;
                }
                fallback = failure[fallback as usize];
            }
            queue.push_back(edge.next);
        }
    }
    (failure, anywhere)
}

pub(super) fn exception_rules() -> impl Iterator<Item = (&'static str, &'static str)> {
    RULES.lines().skip(1).filter_map(|line| {
        // Windows FUN_0041f050 splits at the first ASCII space and copies the
        // entire remaining key. Twelve E rules include a literal trailing space.
        let (tags, text) = line.split_once(' ')?;
        (tags.starts_with('_') && tags.contains('E')).then_some((tags, text))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "Punto trie exceeds u16 node IDs")]
    fn oversized_table_cannot_silently_truncate_node_ids() {
        let mut builder = Builder::new();
        builder.insert(&"a".repeat(u16::MAX as usize), ());
        builder.finish();
    }
}

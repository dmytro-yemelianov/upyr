//! Immutable, contiguous trie storage shared by the complete Punto tables.

use std::borrow::Cow;

pub(super) const ROOT_SLOTS: usize = 128 + 256;

pub(super) fn root_slot(c: char) -> Option<usize> {
    let c = c as u32;
    if c < 128 {
        Some(c as usize)
    } else if (0x400..0x500).contains(&c) {
        Some(128 + (c - 0x400) as usize)
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Node {
    pub(super) edge_start: u16,
    pub(super) value_start: u16,
    pub(super) edge_count: u16,
    pub(super) value_count: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Edge {
    pub(super) symbol: u16,
    pub(super) next: u16,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Trie<T: Copy + 'static> {
    pub(super) nodes: Cow<'static, [Node]>,
    pub(super) edges: Cow<'static, [Edge]>,
    pub(super) values: Cow<'static, [T]>,
    pub(super) root: Cow<'static, [u16]>,
    pub(super) alphabet: Cow<'static, [char]>,
}

impl<T: Copy + 'static> Trie<T> {
    pub(super) const fn from_static(
        nodes: &'static [Node],
        edges: &'static [Edge],
        values: &'static [T],
        root: &'static [u16; ROOT_SLOTS],
        alphabet: &'static [char],
    ) -> Self {
        Self {
            nodes: Cow::Borrowed(nodes),
            edges: Cow::Borrowed(edges),
            values: Cow::Borrowed(values),
            root: Cow::Borrowed(root),
            alphabet: Cow::Borrowed(alphabet),
        }
    }

    pub(super) fn storage_bytes(&self) -> usize {
        std::mem::size_of_val(self.nodes.as_ref())
            + std::mem::size_of_val(self.edges.as_ref())
            + std::mem::size_of_val(self.values.as_ref())
            + std::mem::size_of_val(self.root.as_ref())
            + std::mem::size_of_val(self.alphabet.as_ref())
    }

    pub(super) fn next(&self, node: u16, character: char) -> Option<u16> {
        if node == 0 {
            if let Some(slot) = root_slot(character) {
                let next = self.root[slot];
                return (next != u16::MAX).then_some(next);
            }
        }
        // BMP scalar values fit directly. The unused surrogate range codes
        // supplementary characters from a small side alphabet without loss.
        let symbol = if (character as u32) <= u16::MAX as u32 {
            character as u16
        } else {
            0xd800 + self.alphabet.binary_search(&character).ok()? as u16
        };
        let node = &self.nodes[node as usize];
        let start = node.edge_start as usize;
        let edges = &self.edges[start..start + node.edge_count as usize];
        if edges.len() <= 4 {
            edges
                .iter()
                .find(|edge| edge.symbol == symbol)
                .map(|edge| edge.next)
        } else {
            edges
                .binary_search_by_key(&symbol, |edge| edge.symbol)
                .ok()
                .map(|index| edges[index].next)
        }
    }

    pub(super) fn values(&self, node: u16) -> &[T] {
        let node = &self.nodes[node as usize];
        let start = node.value_start as usize;
        &self.values[start..start + node.value_count as usize]
    }

    pub(super) fn has_children(&self, node: u16) -> bool {
        self.nodes[node as usize].edge_count != 0
    }
}

/// Preserve Unicode lowercase semantics while borrowing already-lowercase text.
pub(super) fn lowercase(text: &str) -> Cow<'_, str> {
    let needs_lowercase = if text.is_ascii() {
        text.bytes().any(|c| c.is_ascii_uppercase())
    } else {
        text.chars().any(|c| match c {
            '\0'..='\u{7f}' => c.is_ascii_uppercase(),
            'а'..='я' | 'ё' => false,
            'А'..='Я' | 'Ё' => true,
            _ => !c.to_lowercase().eq(std::iter::once(c)),
        })
    };
    if needs_lowercase {
        Cow::Owned(text.to_lowercase())
    } else {
        Cow::Borrowed(text)
    }
}

#[cfg(test)]
mod tests {
    use super::super::punto_compile::Builder;
    use super::*;

    #[test]
    fn borrowing_preserves_unicode_and_contextual_lowercase() {
        for text in [
            "lower",
            "UPPER",
            "привіт",
            "ІЇЄҐ",
            "ǅ",
            "İ",
            "ΟΣ",
            "Kµé",
            "😀",
        ] {
            assert_eq!(lowercase(text), text.to_lowercase(), "{text}");
        }
        assert!(matches!(lowercase("привіт"), Cow::Borrowed(_)));
        assert!(matches!(lowercase("lower"), Cow::Borrowed(_)));
    }

    #[test]
    fn direct_root_and_fallback_preserve_distinct_unicode_edges() {
        let mut builder = Builder::new();
        for (i, key) in ["a", "а", "Я", "ї", "№", "😀"].into_iter().enumerate() {
            builder.insert(key, i);
        }
        let trie = builder.finish();
        for (i, c) in ['a', 'а', 'Я', 'ї', '№', '😀'].into_iter().enumerate() {
            assert_eq!(trie.values(trie.next(0, c).unwrap()), &[i]);
        }
        assert_eq!(trie.next(0, 'b'), None);
        assert_eq!(trie.next(0, 'б'), None);
        assert_eq!(trie.next(0, '😄'), None);
    }

    #[test]
    fn compact_edges_preserve_unicode_on_wide_internal_branches() {
        assert_eq!(std::mem::size_of::<Node>(), 8);
        assert_eq!(std::mem::size_of::<Edge>(), 4);
        let keys = ["xa", "xé", "xа", "xЯ", "xї", "x№", "x😀", "x\u{10ffff}"];
        let mut builder = Builder::new();
        for (i, key) in keys.iter().enumerate() {
            builder.insert(key, i);
        }
        let trie = builder.finish();
        for (i, key) in keys.iter().enumerate() {
            let node = key.chars().fold(0, |node, c| trie.next(node, c).unwrap());
            assert_eq!(trie.values(node), &[i]);
        }
        let x = trie.next(0, 'x').unwrap();
        for absent in ['z', 'б', 'ê', '😄'] {
            assert_eq!(trie.next(x, absent), None);
        }
    }
}

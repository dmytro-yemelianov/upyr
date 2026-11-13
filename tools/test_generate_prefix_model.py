import itertools
import unittest

from tools.generate_prefix_model import build_model, lookup


class PrefixModelTests(unittest.TestCase):
    def test_exhaustive_small_language_against_string_prefixes(self):
        words = {"english": {"ab", "abc", "ax", "xb"}, "ukrainian": {"фі", "фс", "чі"}}
        physical = {1: words["english"], 2: {"as", "ac", "xs"}}
        blob, stats = build_model(words)
        for length in range(5):
            for keys in itertools.product("abcsx", repeat=length):
                prefix = "".join(keys)
                expected = 0
                for mask, lexicon in physical.items():
                    if any(word.startswith(prefix) for word in lexicon):
                        expected |= mask
                self.assertEqual(lookup(blob, prefix), expected, prefix)
        self.assertLess(stats["states"], stats["trie_nodes"])

    def test_deterministic_independent_of_input_order(self):
        words = {"english": {"read", "reader", "red"}, "ukrainian": {"виправлення", "виправити"}}
        first, _ = build_model(words)
        second, _ = build_model(dict(reversed(list(words.items()))))
        self.assertEqual(first, second)
        self.assertEqual(lookup(first, "dbgh"), 2)

    def test_unmapped_unicode_cannot_silently_enter_physical_index(self):
        with self.assertRaisesRegex(ValueError, "unmapped"):
            build_model({"english": {"café"}, "ukrainian": set()})


if __name__ == "__main__":
    unittest.main()

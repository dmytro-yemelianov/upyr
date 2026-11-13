import unittest

from tools.generate_context_model import encode, pairs, short_words, HEADER, MAGIC


class ContextModelTests(unittest.TestCase):
    def test_pairs_never_cross_punctuation_or_language_boundaries(self):
        def values(text, lang="english"):
            return [(a, b) for a, b, _, _ in pairs(text, lang)]
        self.assertEqual(values("I have a question."), [("i", "have"), ("have", "a"), ("a", "question")])
        for text in ["I, have", "I-have", "I 123 have", "I_ have", "I я have"]:
            self.assertEqual(values(text), [])
        self.assertEqual(values("то ми тепер", "ukrainian"), [("то", "ми"), ("ми", "тепер")])

    def test_deterministic_encoding_and_frequency_cutoff(self):
        counts = {("i", "have"): 40, ("то", "ми"): 20, ("i", "rare"): 7}
        blob, stats = encode(counts)
        self.assertEqual(blob, encode(dict(reversed(list(counts.items()))))[0])
        magic, words, count, text_bytes = HEADER.unpack_from(blob)
        self.assertEqual((magic, words, count), (MAGIC, 4, 2))
        self.assertEqual(len(blob), HEADER.size + 4 * (words + 1) + text_bytes + 12 * count)
        self.assertEqual(stats["pairs"], 2)

    def test_short_words_are_bounded_and_language_specific(self):
        words = short_words()
        self.assertTrue(all(1 <= len(w) <= 3 for values in words.values() for w in values))
        self.assertIn("i", words["english"])
        self.assertIn("то", words["ukrainian"])


if __name__ == "__main__":
    unittest.main()

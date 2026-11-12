import importlib.util
import tempfile
import unittest
from pathlib import Path


SPEC = importlib.util.spec_from_file_location("import_punto", Path(__file__).with_name("import_punto.py"))
IMPORTER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(IMPORTER)


class ImportPuntoTests(unittest.TestCase):
    def test_rejects_wrong_hash_before_outputs_are_rendered(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary)
            (source / "ps.dat").write_bytes(b"not Punto")
            (source / "triggers.dat").write_bytes(b"not Punto")
            with self.assertRaisesRegex(ValueError, "ps.dat SHA-256"):
                IMPORTER.render(source)

    def test_profiles_do_not_accept_each_others_data(self):
        source = Path(".stos/punto-review/mac-expanded/puntoswitcher.pkg/Payload/PuntoSwitcher.app/Contents/Resources/dictionaries")
        if source.is_dir():
            with self.assertRaisesRegex(ValueError, "ps.dat SHA-256"):
                IMPORTER.render(source, "windows-4.4.2.334")

    def test_decoder_preserves_line_boundaries(self):
        self.assertEqual(IMPORTER.decode_ps(bytes([ord("A") ^ 0xAA, 10, ord("B") ^ 0xAA])), "A\nB")


if __name__ == "__main__":
    unittest.main()

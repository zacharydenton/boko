from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import unittest

import boko


fixtures = Path(__file__).resolve().parents[1] / 'fixtures'


class BindingsTest(unittest.TestCase):
    def test_metadata_for_every_input_format(self):
        for format in ('epub', 'azw3', 'mobi', 'kfx'):
            with self.subTest(format=format):
                data = (fixtures / f'epictetus.{format}').read_bytes()
                info = boko.book_info(data, format)
                self.assertIsInstance(info, dict)
                self.assertEqual(info['title'], 'Short Works')
                self.assertEqual(info['authors'], ['Epictetus'])
                self.assertTrue(info['language'].startswith('en'))
                self.assertGreater(info['chapters'], 0)
                self.assertGreater(info['toc_entries'], 0)

    def test_conversion_roundtrip_for_every_output_format(self):
        data = (fixtures / 'epictetus.epub').read_bytes()
        for format in ('epub', 'azw3', 'kfx'):
            with self.subTest(format=format):
                converted = boko.convert(data, 'epub', format)
                self.assertIsInstance(converted, bytes)
                info = boko.book_info(converted, format)
                self.assertEqual(info['title'], 'Short Works')
                self.assertEqual(info['authors'], ['Epictetus'])
                epub = boko.convert(converted, format, 'epub')
                self.assertEqual(boko.book_info(epub, 'epub')['title'], 'Short Works')

    def test_markdown_and_aliases(self):
        data = (fixtures / 'epictetus.epub').read_bytes()
        markdown = boko.convert(data, from_format='EPUB', to_format='markdown')
        self.assertIsInstance(markdown, bytes)
        self.assertIn('Epictetus', markdown.decode())
        self.assertEqual(markdown, boko.convert(data, 'epub', 'md'))
        mobi = (fixtures / 'epictetus.mobi').read_bytes()
        self.assertEqual(boko.book_info(mobi, 'azw'), boko.book_info(mobi, 'mobi'))

    def test_invalid_formats_are_value_errors(self):
        for from_format, to_format in (('pdf', 'epub'), ('epub', 'pdf'), ('md', 'epub'), ('epub', 'mobi')):
            with self.subTest(from_format=from_format, to_format=to_format):
                with self.assertRaises(ValueError):
                    boko.convert(b'', from_format, to_format)
        for format in ('pdf', 'markdown'):
            with self.subTest(format=format), self.assertRaises(ValueError):
                boko.book_info(b'', format)

    def test_malformed_input_is_a_value_error(self):
        for format in ('epub', 'azw3', 'mobi', 'kfx'):
            for data in (b'', b'not an ebook', bytes(100)):
                with self.subTest(format=format, data=data):
                    with self.assertRaises(ValueError):
                        boko.book_info(data, format)
                    with self.assertRaises(ValueError):
                        boko.convert(data, format, 'epub')

    def test_input_must_be_bytes(self):
        with self.assertRaises(TypeError):
            boko.convert('not bytes', 'epub', 'kfx')
        with self.assertRaises(TypeError):
            boko.book_info('not bytes', 'epub')

    def test_concurrent_conversion(self):
        data = (fixtures / 'epictetus.epub').read_bytes()
        with ThreadPoolExecutor(max_workers=2) as executor:
            results = list(executor.map(lambda _: boko.convert(data, 'epub', 'md'), range(4)))
        self.assertTrue(results[0])
        self.assertTrue(all(result == results[0] for result in results))


if __name__ == '__main__':
    unittest.main()

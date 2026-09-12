"""Tests for the source-type classifier and extractability axis.

Run: python3 -m unittest discover -s scripts/eval-corpus
"""

import unittest

import sourcetype as st


class TestClassify(unittest.TestCase):
    def test_news_hosts(self):
        for url in [
            "https://www.nytimes.com/2020/01/01/world/story.html",
            "http://bbc.co.uk/news/uk-12345",
            "https://www.theguardian.com/world/2019/may/01/x",
        ]:
            self.assertEqual(st.classify(url), st.NEWS, url)

    def test_subdomain_matches_registrable_host(self):
        self.assertEqual(st.classify("https://api.nytimes.com/x"), st.NEWS)
        self.assertEqual(st.classify("https://link.springer.com/article/1"), st.JOURNAL)

    def test_reference_hosts(self):
        self.assertEqual(st.classify("https://en.wikipedia.org/wiki/X"), st.REFERENCE)
        self.assertEqual(st.classify("https://www.britannica.com/topic/x"), st.REFERENCE)

    def test_journal_beats_gov_suffix_for_ncbi(self):
        # pubmed lives under *.nih.gov but is a journal index, not a gov page.
        self.assertEqual(
            st.classify("https://pubmed.ncbi.nlm.nih.gov/31452104/"), st.JOURNAL
        )

    def test_gov_suffixes(self):
        self.assertEqual(st.classify("https://www.census.gov/data"), st.GOV)
        self.assertEqual(st.classify("https://www.gov.uk/government/x"), st.GOV)
        self.assertEqual(st.classify("https://defense.mil/news"), st.GOV)

    def test_books_classify_as_book(self):
        for url in [
            "https://books.google.com/books?id=abc",
            "https://openlibrary.org/books/OL1M",
            "https://archive.org/details/somebook",
        ]:
            self.assertEqual(st.classify(url), st.BOOK, url)

    def test_blog_hosts(self):
        self.assertEqual(st.classify("https://someone.medium.com/post"), st.BLOG)
        self.assertEqual(st.classify("https://x.substack.com/p/y"), st.BLOG)

    def test_unknown_host_is_other(self):
        self.assertEqual(st.classify("https://some-random-site.example/x"), st.OTHER)

    def test_unparseable_url_is_other(self):
        self.assertEqual(st.classify("not a url"), st.OTHER)
        self.assertEqual(st.classify(""), st.OTHER)

    def test_every_result_is_a_known_type(self):
        for url in [
            "https://nytimes.com",
            "https://books.google.com/b",
            "https://x.example",
            "garbage",
        ]:
            self.assertIn(st.classify(url), st.SOURCE_TYPES)


class TestExtractability(unittest.TestCase):
    def test_books_are_queued(self):
        self.assertEqual(st.extractability(st.BOOK), st.QUEUED)

    def test_generic_html_types_are_extractable(self):
        for t in (st.NEWS, st.REFERENCE, st.JOURNAL, st.GOV, st.BLOG, st.OTHER):
            self.assertEqual(st.extractability(t), st.EXTRACTABLE, t)

    def test_extractable_set_is_subset_of_source_types(self):
        self.assertTrue(set(st.EXTRACTABLE_SOURCE_TYPES).issubset(set(st.SOURCE_TYPES)))


if __name__ == "__main__":
    unittest.main()

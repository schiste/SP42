"""Tests for the WAFER record parser, against the synthetic fixture
(shaped to the real wafer-fail-dev schema verified 2026-06-23).

Run: python3 -m unittest discover -s scripts/eval-corpus
"""

import os
import unittest

import wafer

FIXTURE = os.path.join(os.path.dirname(__file__), "fixtures", "wafer-sample.jsonl")


class TestHelpers(unittest.TestCase):
    def test_clean_section(self):
        self.assertEqual(wafer.clean_section("Section::::Biography."), "Biography")
        self.assertEqual(wafer.clean_section("Section::::History"), "History")
        self.assertIsNone(wafer.clean_section(None))

    def test_split_input(self):
        title, body = wafer.split_input("T [SEP] Section::::S. [SEP] a [CIT] b")
        self.assertEqual(title, "T")
        self.assertIn("[CIT]", body)
        self.assertNotIn("[SEP]", body)

    def test_claim_before_cit_takes_last_sentence(self):
        body = "First sentence.\n Second sentence here. [CIT]"
        self.assertEqual(wafer.claim_before_cit(body), "Second sentence here.")

    def test_claim_before_cit_fallback(self):
        self.assertEqual(wafer.claim_before_cit("", fallback="fb"), "fb")

    def test_normalize_url_adds_scheme(self):
        self.assertEqual(wafer.normalize_url("bbc.co.uk/x"), "https://bbc.co.uk/x")
        self.assertEqual(
            wafer.normalize_url("https://bbc.co.uk/x"), "https://bbc.co.uk/x"
        )

    def test_select_claim_is_last_sentence(self):
        claim, ctx = wafer.select_claim_and_context(["A.", "B.", "C is the claim."])
        self.assertEqual(claim, "C is the claim.")
        self.assertEqual(ctx, ["A.", "B."])

    def test_select_claim_drops_structural_markers(self):
        claim, ctx = wafer.select_claim_and_context(
            ["Title", "Section::::Other.", "BULLET::::-", "The real claim."]
        )
        self.assertEqual(claim, "The real claim.")
        self.assertEqual(ctx, ["Title"])

    def test_select_claim_empty(self):
        self.assertEqual(wafer.select_claim_and_context([]), ("", []))


class TestParseRecord(unittest.TestCase):
    def setUp(self):
        self.cases = list(wafer.iter_jsonl(FIXTURE))

    def test_parses_all_records(self):
        self.assertEqual(len(self.cases), 4)

    def test_positive_uses_answer_url_not_candidate(self):
        # record 4: answer is the gold url; provenance[0] is a different candidate
        c = self.cases[3]
        self.assertEqual(c.sources[0].url, "https://nature.com/articles/xyz")
        self.assertNotIn("random-candidate", c.sources[0].url)

    def test_categories_and_wikipedia_url(self):
        c = self.cases[3]
        self.assertIn("Climate change", c.categories)
        self.assertEqual(c.wikipedia_url, "https://en.wikipedia.org/wiki/Climate_Topic")
        self.assertEqual(self.cases[1].categories, [])  # fail-dev record has none

    def test_categories_split_preserves_internal_comma(self):
        # "...,Scientists from Nashville, Tennessee,..." must stay one category
        cats = self.cases[3].categories
        self.assertIn("Scientists from Nashville, Tennessee", cats)
        self.assertEqual(cats, ["Climate change", "Scientists from Nashville, Tennessee", "Environmental science"])

    def test_title_and_section(self):
        c = self.cases[0]
        self.assertEqual(c.title, "Example Bridge")
        self.assertEqual(c.section, "History")

    def test_claim_is_sentence_before_cit(self):
        c = self.cases[0]
        self.assertNotIn("[CIT]", c.claim_text)
        self.assertTrue(c.claim_text.startswith("The bridge opened to traffic in 1937"))

    def test_context_window_excludes_claim(self):
        # record 0 has 2 sentences; the last is the claim, leaving 1 of context
        c = self.cases[0]
        self.assertEqual(len(c.context_sentences), 1)
        self.assertEqual(c.context_sentences, ["It was funded by a public bond."])
        self.assertNotIn(c.claim_text, c.context_sentences)

    def test_featured_present_and_absent(self):
        self.assertTrue(self.cases[0].featured)   # has featured: true
        self.assertFalse(self.cases[1].featured)  # no featured key -> False

    def test_source_url_from_provenance(self):
        c = self.cases[0]
        self.assertEqual(len(c.sources), 1)
        self.assertIn("nytimes.com", c.sources[0].url)
        self.assertEqual(len(c.sources[0].chunk_ids), 2)  # two provenance chunks
        self.assertIsNone(c.sources[0].snapshot_text)  # not inline in fail-dev shape

    def test_failed_verification_flag(self):
        self.assertTrue(self.cases[1].sources[0].failed_verification)
        self.assertFalse(self.cases[0].sources[0].failed_verification)

    def test_split_drives_is_fail_split(self):
        cases = list(wafer.iter_jsonl(FIXTURE, split="fail-dev"))
        self.assertTrue(all(c.is_fail_split for c in cases))
        self.assertFalse(self.cases[0].is_fail_split)


class TestParseCategories(unittest.TestCase):
    def test_single(self):
        self.assertEqual(wafer.parse_categories("One category"), ["One category"])

    def test_joined_with_internal_comma(self):
        self.assertEqual(
            wafer.parse_categories("A,Nashville, Tennessee,B"),
            ["A", "Nashville, Tennessee", "B"],
        )

    def test_empty_and_list(self):
        self.assertEqual(wafer.parse_categories(None), [])
        self.assertEqual(wafer.parse_categories(""), [])
        self.assertEqual(wafer.parse_categories(["X", " Y "]), ["X", "Y"])


if __name__ == "__main__":
    unittest.main()

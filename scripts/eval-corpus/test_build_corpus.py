"""Tests for the WAFER build pipeline (PRD-0007 schema), against the fixture.

Run: python3 -m unittest discover -s scripts/eval-corpus
"""

import os
import unittest

import build_corpus as bc
import sourcetype as st

FIXTURE = os.path.join(os.path.dirname(__file__), "fixtures", "wafer-sample.jsonl")


def stype(c):
    return st.classify(c.source_url)


class TestPipeline(unittest.TestCase):
    def setUp(self):
        self.sampled, self.queued, self.manifest = bc.run(
            FIXTURE, split=None, per_stratum=50, seed=1
        )

    def test_failed_verification_becomes_not_supported(self):
        book = [c for c in self.queued if stype(c) == st.BOOK]
        self.assertEqual(len(book), 1)
        self.assertEqual(book[0].expected_outcome, bc.NOT_SUPPORTED)
        self.assertEqual(book[0].label_method, "wafer-fail-split")

    def test_book_is_queued_not_active(self):
        self.assertNotIn(st.BOOK, [stype(c) for c in self.sampled])
        self.assertEqual(self.manifest["counts"]["queued_total"], 1)

    def test_active_cases_are_supported_positives(self):
        self.assertEqual(len(self.sampled), 3)
        self.assertTrue(all(c.expected_outcome == bc.SUPPORTED for c in self.sampled))
        self.assertEqual({stype(c) for c in self.sampled}, {st.NEWS, st.REFERENCE, st.JOURNAL})

    def test_computed_facets_not_stored_in_case(self):
        # source_type / extractability are computed, must not be in the case
        c = self.sampled[0]
        self.assertNotIn("source_type", c.cohorts)
        self.assertNotIn("extractability", c.cohorts)

    def test_inherent_cohorts_and_context(self):
        news = next(c for c in self.sampled if stype(c) == st.NEWS)
        self.assertEqual(len(news.claim_context["context_sentences"]), 1)
        self.assertTrue(news.cohorts["featured"])
        self.assertEqual(news.cohorts["language"], "en")
        self.assertIsNone(news.source_text)  # WAFER has no inline text

    def test_label_provenance(self):
        c = self.sampled[0]
        self.assertIn(c.label_method, ("wafer-distant-supervision", "wafer-fail-split"))
        self.assertEqual(c.label_as_of, bc.WAFER_AS_OF)

    def test_licensing_on_claim(self):
        c = self.sampled[0]
        self.assertEqual(c.licensing["claim"]["license"], "CC-BY-SA-4.0")
        self.assertEqual(c.licensing["claim"]["source"], "Wikipedia")

    def test_categories_in_cohorts_for_bias_set(self):
        journal = next(c for c in self.sampled if stype(c) == st.JOURNAL)
        self.assertIn("Climate change", journal.cohorts["categories"])

    def test_no_banned_keys(self):
        d = bc.case_to_dict(self.sampled[0])
        for banned in ("confidence", "tranche", "dataset_version"):
            self.assertNotIn(banned, d)
        self.assertTrue(d["id"].startswith("cit-"))  # content-hash, not a row number


class TestStratifiedSampleDeterminism(unittest.TestCase):
    def _many(self, n):
        return [
            bc.EmittedCase(
                id=bc.case_id(f"claim {i}", f"https://nytimes.com/{i}"),
                claim=f"claim {i}",
                claim_context={},
                source_url=f"https://nytimes.com/{i}",
                expected_outcome=bc.SUPPORTED,
                label_method="wafer-distant-supervision",
                label_as_of=bc.WAFER_AS_OF,
                cohorts={"language": "en"},
                licensing={},
                provenance={},
            )
            for i in range(n)
        ]

    def test_same_seed_same_sample(self):
        cases = self._many(20)
        a = bc.stratified_sample(cases, per_stratum=5, seed=7)
        b = bc.stratified_sample(cases, per_stratum=5, seed=7)
        self.assertEqual([c.id for c in a], [c.id for c in b])
        self.assertEqual(len(a), 5)

    def test_different_seed_can_differ(self):
        cases = self._many(20)
        a = bc.stratified_sample(cases, per_stratum=5, seed=7)
        b = bc.stratified_sample(cases, per_stratum=5, seed=8)
        self.assertNotEqual([c.id for c in a], [c.id for c in b])

    def test_caps_per_stratum(self):
        self.assertEqual(len(bc.stratified_sample(self._many(20), per_stratum=3, seed=1)), 3)


if __name__ == "__main__":
    unittest.main()

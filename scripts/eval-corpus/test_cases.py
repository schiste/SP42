"""Tests for the shared case schema helpers (cases.py)."""

import unittest

import cases as cs


class TestCaseId(unittest.TestCase):
    def test_stable_and_content_addressed(self):
        a = cs.case_id("claim x", "https://a.example")
        self.assertEqual(a, cs.case_id("claim x", "https://a.example"))
        self.assertNotEqual(a, cs.case_id("claim y", "https://a.example"))
        self.assertTrue(a.startswith("cit-"))


class TestRevisionFromUrl(unittest.TestCase):
    def test_extracts_oldid(self):
        self.assertEqual(
            cs.revision_from_url(
                "https://en.wikipedia.org/w/index.php?title=X&oldid=1331476438"
            ),
            "1331476438",
        )

    def test_none_when_absent(self):
        self.assertIsNone(cs.revision_from_url("https://en.wikipedia.org/wiki/X"))
        self.assertIsNone(cs.revision_from_url(""))


class TestLicensing(unittest.TestCase):
    def test_wikipedia_is_cc_by_sa_with_revision(self):
        lic = cs.wikipedia_licensing(
            "X", "https://en.wikipedia.org/w/index.php?oldid=99", "42"
        )
        self.assertEqual(lic["license"], cs.CC_BY_SA)
        self.assertEqual(lic["revision"], "99")
        self.assertEqual(lic["wikipedia_id"], "42")

    def test_fair_use_carries_origin(self):
        lic = cs.fair_use_licensing("https://news.example/a")
        self.assertEqual(lic["license"], cs.FAIR_USE)
        self.assertEqual(lic["origin_url"], "https://news.example/a")


class TestCaseToDict(unittest.TestCase):
    def _case(self, source_text):
        return cs.EmittedCase(
            id="cit-x",
            claim="c",
            claim_context={},
            source_url="u",
            expected_outcome=cs.SUPPORTED,
            label_method="m",
            label_as_of="2026-01-01",
            cohorts={},
            licensing={},
            source_text=source_text,
        )

    def test_drops_null_source_text(self):
        self.assertNotIn("source_text", cs.case_to_dict(self._case(None)))

    def test_keeps_present_source_text(self):
        self.assertEqual(cs.case_to_dict(self._case("body"))["source_text"], "body")


if __name__ == "__main__":
    unittest.main()

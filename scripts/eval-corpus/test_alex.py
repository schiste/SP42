"""Tests for the alex-cite-checker importer (alex.py)."""

import unittest

import alex
import cases as cs


def _row(**over):
    row = {
        "id": "row_2",
        "article_url": "https://en.wikipedia.org/w/index.php?title=X&oldid=1331476438",
        "article_title": "X",
        "citation_number": 1,
        "claim_text": "A claim about X.",
        "claim_container": "A claim about X.[1] More text.",
        "source_url": "https://news.example/story",
        "source_text": "The source says A is true.",
        "ground_truth": "Supported",
        "dataset_version": "v9",
    }
    row.update(over)
    return row


class TestRowToCase(unittest.TestCase):
    def test_maps_ground_truth(self):
        self.assertEqual(alex.row_to_case(_row()).expected_outcome, cs.SUPPORTED)
        self.assertEqual(
            alex.row_to_case(_row(ground_truth="Partially supported")).expected_outcome,
            cs.PARTIAL,
        )
        self.assertEqual(
            alex.row_to_case(_row(ground_truth="Not supported")).expected_outcome,
            cs.NOT_SUPPORTED,
        )

    def test_content_hash_id_not_row_number(self):
        c = alex.row_to_case(_row())
        self.assertTrue(c.id.startswith("cit-"))
        self.assertNotEqual(c.id, "row_2")
        self.assertEqual(c.provenance["alex_id"], "row_2")  # original kept in provenance

    def test_no_banned_version_token(self):
        d = cs.case_to_dict(alex.row_to_case(_row()))
        self.assertNotIn("dataset_version", d)
        self.assertNotIn("dataset_version", d.get("provenance", {}))

    def test_source_text_is_fair_use(self):
        c = alex.row_to_case(_row())
        self.assertEqual(c.source_text, "The source says A is true.")
        self.assertEqual(c.licensing["source_text"]["license"], cs.FAIR_USE)

    def test_claim_is_cc_by_sa_with_revision(self):
        c = alex.row_to_case(_row())
        self.assertEqual(c.licensing["claim"]["license"], cs.CC_BY_SA)
        self.assertEqual(c.licensing["claim"]["revision"], "1331476438")

    def test_label_provenance(self):
        c = alex.row_to_case(_row())
        self.assertEqual(c.label_method, "alex-cite-checker-curated")
        self.assertEqual(c.label_as_of, alex.ALEX_AS_OF)

    def test_unknown_gt_dropped(self):
        self.assertIsNone(alex.row_to_case(_row(ground_truth="???")))


if __name__ == "__main__":
    unittest.main()

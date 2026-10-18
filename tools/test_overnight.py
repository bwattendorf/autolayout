"""Tests for tools/overnight.py's scoring. Run under KiCad's Python (overnight imports pcbnew):

    "C:/Program Files/KiCad/10.0/bin/python.exe" tools/test_overnight.py
"""

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import overnight  # noqa: E402


def row(unconnected, clearance=0, shorting=0, h2h=0, before=(0, 0, 0), status="ok"):
    return {
        "status": status,
        "kicad_unconnected_after": unconnected,
        "kicad_clearance": clearance,
        "kicad_shorting_items": shorting,
        "kicad_hole_to_hole": h2h,
        "kicad_clearance_before": before[0],
        "kicad_shorting_items_before": before[1],
        "kicad_hole_to_hole_before": before[2],
    }


class ScoringTests(unittest.TestCase):
    def test_zero_unconnected_is_a_real_count_and_solves_the_board(self):
        # The bug this guards: `int(x or 10**9)` read 0 as "missing" (2026-10-17, c4 candidate3).
        self.assertEqual(overnight.key_of(row(0)), (0, 0))
        self.assertTrue(overnight.solved(row(0)))

    def test_violations_the_board_already_had_do_not_count(self):
        r = row(0, clearance=3, shorting=1, h2h=2, before=(3, 1, 2))
        self.assertEqual(overnight.key_of(r), (0, 0))
        self.assertTrue(overnight.solved(r))

    def test_new_violations_stop_a_board_counting_as_solved(self):
        r = row(0, clearance=5, before=(3, 0, 0))
        self.assertEqual(overnight.key_of(r), (0, 2))
        self.assertFalse(overnight.solved(r))

    def test_fewer_unconnected_beats_fewer_violations(self):
        self.assertLess(overnight.key_of(row(1, clearance=9)), overnight.key_of(row(2)))

    def test_a_failed_attempt_is_worse_than_any_result(self):
        failed = {"status": "FAIL:route", "error": "x"}
        self.assertGreater(overnight.key_of(failed), overnight.key_of(row(400)))
        self.assertFalse(overnight.solved(failed))
        # An ok row with the count missing is not a solved board either.
        self.assertFalse(overnight.solved({"status": "ok"}))


if __name__ == "__main__":
    unittest.main()

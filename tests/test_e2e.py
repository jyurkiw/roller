"""
End-to-end tests for the roller Python extension module.

These tests import the compiled Rust library via PyO3 and exercise the
public Python API with real randomness — no mocking. Every assertion is
based on mathematical bounds or structural invariants, not exact values.

Run:
    pip install maturin
    maturin develop          # compile + install into current venv
    pytest tests/ -v         # or: python -m pytest tests/ -v
"""

import unittest

from roller import Roll, RollEntry


# =========================================================================
#  Construction & basic attributes
# =========================================================================
class TestConstruction(unittest.TestCase):
    """Verify that Roll objects are created correctly and expose the
    expected attributes."""

    def test_code_is_stored(self):
        r = Roll("1d20")
        self.assertEqual(r.code, "1d20")

    def test_total_is_int(self):
        r = Roll("1d6")
        self.assertIsInstance(r.total, int)

    def test_total_static_is_int(self):
        r = Roll("1d6+5")
        self.assertIsInstance(r.total_static, int)

    def test_rolls_is_list(self):
        r = Roll("1d20")
        self.assertIsInstance(r.rolls, list)

    def test_roll_entry_has_label_and_details(self):
        r = Roll("2d6")
        entry = r.rolls[0]
        self.assertIsInstance(entry, RollEntry)
        self.assertIsInstance(entry.label, str)
        self.assertIsInstance(entry.details, list)

    def test_repr_contains_code(self):
        r = Roll("3d8+2")
        self.assertIn("3d8+2", repr(r))


# =========================================================================
#  Bounds checking — real randomness, math-guaranteed ranges
# =========================================================================
class TestBounds(unittest.TestCase):
    """Every die roll must fall within [1, sides].  We verify the total
    is within the theoretical min/max for the expression."""

    def test_1d6_bounds(self):
        for _ in range(200):
            r = Roll("1d6")
            self.assertGreaterEqual(r.total, 1)
            self.assertLessEqual(r.total, 6)

    def test_2d8_bounds(self):
        for _ in range(200):
            r = Roll("2d8")
            # 2d8: min=2, max=16
            self.assertGreaterEqual(r.total, 2)
            self.assertLessEqual(r.total, 16)

    def test_1d20_plus5_bounds(self):
        for _ in range(200):
            r = Roll("1d20+5")
            # 1d20+5: min=6, max=25
            self.assertGreaterEqual(r.total, 6)
            self.assertLessEqual(r.total, 25)

    def test_1d6_minus1_bounds(self):
        for _ in range(200):
            r = Roll("1d6-1")
            # 1d6-1: min=0, max=5
            self.assertGreaterEqual(r.total, 0)
            self.assertLessEqual(r.total, 5)

    def test_4d6kh3_bounds(self):
        for _ in range(200):
            r = Roll("4d6kh3")
            # keep best 3 of 4d6: min=3, max=18
            self.assertGreaterEqual(r.total, 3)
            self.assertLessEqual(r.total, 18)

    def test_2d20kl1_bounds(self):
        for _ in range(200):
            r = Roll("2d20kl1")
            # keep lowest of 2d20: min=1, max=20
            self.assertGreaterEqual(r.total, 1)
            self.assertLessEqual(r.total, 20)


# =========================================================================
#  Static modifier handling
# =========================================================================
class TestStaticModifiers(unittest.TestCase):

    def test_positive_static(self):
        r = Roll("1d6+3")
        self.assertEqual(r.total_static, 3)

    def test_negative_static(self):
        r = Roll("1d6-2")
        self.assertEqual(r.total_static, -2)

    def test_multiple_statics_accumulate(self):
        r = Roll("1d20+5-2")
        self.assertEqual(r.total_static, 3)

    def test_static_included_in_total(self):
        r = Roll("1d6+100")
        # Even the worst roll (1) + 100 = 101
        self.assertGreaterEqual(r.total, 101)


# =========================================================================
#  Structural invariants for compound expressions
# =========================================================================
class TestCompoundExpressions(unittest.TestCase):

    def test_1d20_plus5_plus3d6kh1_structure(self):
        r = Roll("1d20+5+3d6kh1")
        self.assertEqual(r.total_static, 5)
        self.assertEqual(len(r.rolls), 2)
        # First group: 1d20, keeps 1 die
        self.assertEqual(r.rolls[0].label, "1d20")
        self.assertEqual(len(r.rolls[0].details), 1)
        # Second group: 3d6kh1, keeps 1 die
        self.assertEqual(r.rolls[1].label, "3d6")
        self.assertEqual(len(r.rolls[1].details), 1)

    def test_2d8_minus2_plus4d6kh3_structure(self):
        r = Roll("2d8-2+4d6kh3")
        self.assertEqual(r.total_static, -2)
        self.assertEqual(len(r.rolls), 2)
        # First group: 2d8, no keep → all 2 dice
        self.assertEqual(r.rolls[0].label, "2d8")
        self.assertEqual(len(r.rolls[0].details), 2)
        # Second group: 4d6kh3, keeps 3
        self.assertEqual(r.rolls[1].label, "4d6")
        self.assertEqual(len(r.rolls[1].details), 3)

    def test_compound_total_equals_parts(self):
        """total must always equal sum(all details) + total_static."""
        codes = [
            "1d20+5+3d6kh1",
            "2d8-2+4d6kh3",
            "d20+d8+5-1",
            "2d20kl1+4d6kh3-3",
            "1d20+5-2d6",
        ]
        for code in codes:
            for _ in range(50):
                r = Roll(code)
                detail_sum = sum(
                    val for entry in r.rolls for val in entry.details
                )
                self.assertEqual(
                    r.total,
                    detail_sum + r.total_static,
                    msg=f"Invariant broken for '{code}': "
                        f"total={r.total}, details={detail_sum}, "
                        f"static={r.total_static}",
                )

    def test_subtracted_dice_are_negative(self):
        """When a dice group is subtracted (e.g. -2d6), kept details
        should be negative."""
        for _ in range(100):
            r = Roll("1d20+5-2d6")
            for val in r.rolls[1].details:
                self.assertLess(val, 0, msg=f"Expected negative detail, got {val}")

    def test_subtracted_dice_bounds(self):
        for _ in range(200):
            r = Roll("1d20+5-2d6")
            # min: 1 + 5 - 12 = -6,  max: 20 + 5 - 2 = 23
            self.assertGreaterEqual(r.total, -6)
            self.assertLessEqual(r.total, 23)


# =========================================================================
#  Keep mechanics (structural checks)
# =========================================================================
class TestKeepMechanics(unittest.TestCase):

    def test_keep_high_sorts_descending(self):
        """Kept details for kh should be in descending order."""
        for _ in range(100):
            r = Roll("4d6kh3")
            details = r.rolls[0].details
            self.assertEqual(details, sorted(details, reverse=True))

    def test_keep_low_sorts_ascending(self):
        """Kept details for kl should be in ascending order."""
        for _ in range(100):
            r = Roll("4d6kl1")
            details = r.rolls[0].details
            self.assertEqual(details, sorted(details))

    def test_keep_count_respected(self):
        r = Roll("5d10kh2")
        self.assertEqual(len(r.rolls[0].details), 2)

    def test_no_keep_retains_all(self):
        r = Roll("3d8")
        self.assertEqual(len(r.rolls[0].details), 3)


# =========================================================================
#  Reroll
# =========================================================================
class TestReroll(unittest.TestCase):

    def test_reroll_returns_int(self):
        r = Roll("1d20")
        result = r.reroll()
        self.assertIsInstance(result, int)

    def test_reroll_updates_total(self):
        """Over many rerolls the total should vary (not be stuck)."""
        r = Roll("1d20")
        totals = {r.total}
        for _ in range(100):
            r.reroll()
            totals.add(r.total)
        # With 1d20 over 100 rerolls, seeing only 1 unique value is
        # astronomically unlikely (~5e-131).
        self.assertGreater(len(totals), 1)

    def test_reroll_preserves_code(self):
        r = Roll("2d6+3")
        r.reroll()
        self.assertEqual(r.code, "2d6+3")

    def test_reroll_clears_previous_state(self):
        r = Roll("4d6kh3")
        r.reroll()
        # Should still have exactly 1 roll group with 3 kept dice
        self.assertEqual(len(r.rolls), 1)
        self.assertEqual(len(r.rolls[0].details), 3)


# =========================================================================
#  Error handling
# =========================================================================
class TestErrors(unittest.TestCase):

    def test_invalid_code_raises_value_error(self):
        with self.assertRaises(ValueError):
            Roll("zzz")

    def test_empty_string_raises_value_error(self):
        with self.assertRaises(ValueError):
            Roll("")

    def test_error_message_contains_code(self):
        try:
            Roll("bad_code")
        except ValueError as e:
            self.assertIn("bad_code", str(e))


# =========================================================================
#  Statistical smoke tests
# =========================================================================
class TestStatisticalSmoke(unittest.TestCase):
    """Light statistical checks — not rigorous hypothesis tests, but
    enough to catch a broken RNG or off-by-one in bounds."""

    def test_1d6_mean_is_reasonable(self):
        """E[1d6] = 3.5.  Over 5000 samples the mean should be close."""
        totals = [Roll("1d6").total for _ in range(5000)]
        mean = sum(totals) / len(totals)
        self.assertAlmostEqual(mean, 3.5, delta=0.2)

    def test_1d20_covers_full_range(self):
        """Over 2000 rolls of 1d20, every face 1-20 should appear at
        least once (prob of missing any single face ≈ 0)."""
        seen = set()
        for _ in range(2000):
            seen.add(Roll("1d20").total)
        self.assertEqual(seen, set(range(1, 21)))

    def test_4d6kh3_mean_is_reasonable(self):
        """E[4d6kh3] ≈ 12.24.  Generous delta for sample noise."""
        totals = [Roll("4d6kh3").total for _ in range(5000)]
        mean = sum(totals) / len(totals)
        self.assertAlmostEqual(mean, 12.24, delta=0.3)


if __name__ == "__main__":
    unittest.main()
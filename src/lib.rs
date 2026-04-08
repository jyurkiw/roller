use once_cell::sync::Lazy;
use rand::Rng;
use regex::Regex;

// ---------------------------------------------------------------------------
//  Regex: matches die groups like "2d6", "+1d4kh3", "-3", "d20kl1"
// ---------------------------------------------------------------------------
static ROLL_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[+-]?\d*d\d+k?[hl]?\d?|[+-]\d+").expect("invalid roll pattern regex")
});

// ---------------------------------------------------------------------------
//  Error type
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RollError {
    InvalidDieCode(String),
    InvalidStaticCode(String),
}

impl std::fmt::Display for RollError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RollError::InvalidDieCode(c) => write!(f, "Invalid die code: {c}"),
            RollError::InvalidStaticCode(c) => write!(f, "Invalid static code: {c}"),
        }
    }
}

impl std::error::Error for RollError {}

// ---------------------------------------------------------------------------
//  Core types
// ---------------------------------------------------------------------------
#[derive(Debug, Clone)]
pub struct RollEntry {
    pub label: String,
    pub details: Vec<i64>,
}

#[derive(Debug, Clone)]
pub struct Roll {
    pub code: String,
    pub rolls: Vec<RollEntry>,
    pub total_static: i64,
}

// ---------------------------------------------------------------------------
//  Pure parsing helpers (public for unit-testing)
// ---------------------------------------------------------------------------

/// Returns (sign_char, stripped_code).
pub fn get_roll_type(code: &str) -> (char, &str) {
    if let Some(rest) = code.strip_prefix('-') {
        ('-', rest)
    } else if let Some(rest) = code.strip_prefix('+') {
        ('+', rest)
    } else {
        ('+', code)
    }
}

/// Returns (keep_high, keep_num, stripped_code).
/// A keep_num of 0 means "keep all dice" (no k token present).
pub fn get_keep_meta(code: &str) -> (bool, usize, &str) {
    if let Some(k_pos) = code.find('k') {
        let after_k = &code[k_pos + 1..];
        let keep_high = after_k.starts_with('h');
        let keep_num: usize = after_k
            .chars()
            .last()
            .and_then(|c| c.to_digit(10))
            .map(|d| d as usize)
            .unwrap_or(1);
        let base = &code[..k_pos];
        (keep_high, keep_num, base)
    } else {
        // 0 signals "keep all" — resolved in roll() once we know num
        (true, 0, code)
    }
}

/// Returns (num_dice, sides).
pub fn get_num_and_sides(code: &str) -> (usize, usize) {
    let mut parts = code.split('d');
    let num_str = parts.next().unwrap_or("");
    let sides_str = parts.next().unwrap_or("0");
    let num: usize = if num_str.is_empty() {
        1
    } else {
        num_str.parse().unwrap_or(1)
    };
    let sides: usize = sides_str.parse().unwrap_or(0);
    (num, sides)
}

/// Parse a pure static modifier like "+5", "-3", or "7".
pub fn parse_code_static(code: &str) -> Result<i64, RollError> {
    if code.contains('d') {
        return Err(RollError::InvalidStaticCode(code.to_string()));
    }
    let (sign, digits) = if let Some(rest) = code.strip_prefix('-') {
        (-1i64, rest)
    } else if let Some(rest) = code.strip_prefix('+') {
        (1i64, rest)
    } else {
        (1i64, code)
    };
    digits
        .parse::<i64>()
        .map(|v| sign * v)
        .map_err(|_| RollError::InvalidStaticCode(code.to_string()))
}

/// Find all regex tokens in a die code string.
pub fn find_matches(code: &str) -> Vec<String> {
    ROLL_PATTERN
        .find_iter(code)
        .map(|m: regex::Match| m.as_str().to_string())
        .collect()
}

// ---------------------------------------------------------------------------
//  Dice rolling (trait-based for mockability)
// ---------------------------------------------------------------------------

/// Trait abstracting the random number source so tests can inject
/// deterministic results.
pub trait DiceRoller {
    fn roll_dice(&mut self, num: usize, sides: usize) -> Vec<i64>;
}

/// Production roller backed by `rand`.
pub struct RandomDiceRoller;

impl DiceRoller for RandomDiceRoller {
    fn roll_dice(&mut self, num: usize, sides: usize) -> Vec<i64> {
        let mut rng = rand::thread_rng();
        (0..num).map(|_| rng.gen_range(1..=sides as i64)).collect()
    }
}

// ---------------------------------------------------------------------------
//  Roll implementation
// ---------------------------------------------------------------------------
impl Roll {
    /// Create a new Roll, immediately evaluating it with the given roller.
    pub fn new(die_code: &str, roller: &mut dyn DiceRoller) -> Result<Self, RollError> {
        let mut roll = Roll {
            code: die_code.to_string(),
            rolls: Vec::new(),
            total_static: 0,
        };
        roll.roll(roller)?;
        Ok(roll)
    }

    /// Convenience constructor using real randomness.
    pub fn new_random(die_code: &str) -> Result<Self, RollError> {
        Self::new(die_code, &mut RandomDiceRoller)
    }

    pub fn clear(&mut self) {
        self.rolls.clear();
        self.total_static = 0;
    }

    pub fn total(&self) -> i64 {
        let dice_sum: i64 = self
            .rolls
            .iter()
            .flat_map(|entry| entry.details.iter())
            .sum();
        dice_sum + self.total_static
    }

    pub fn roll(&mut self, roller: &mut dyn DiceRoller) -> Result<i64, RollError> {
        self.clear();
        let matches = find_matches(&self.code);
        if matches.is_empty() {
            return Err(RollError::InvalidDieCode(self.code.clone()));
        }

        for token in &matches {
            if !token.contains('d') {
                self.total_static += parse_code_static(token)?;
            } else {
                let (sign, code) = get_roll_type(token);
                let (keep_high, keep_num, code) = get_keep_meta(code);
                let (num, sides) = get_num_and_sides(code);

                // keep_num == 0 means "no k token" → keep all dice
                let keep_num = if keep_num == 0 { num } else { keep_num };

                let mut dice = roller.roll_dice(num, sides);
                dice.sort();
                if keep_high {
                    dice.reverse();
                }

                let sign_mul: i64 = if sign == '-' { -1 } else { 1 };
                let kept: Vec<i64> = dice
                    .into_iter()
                    .take(keep_num)
                    .map(|v| v * sign_mul)
                    .collect();
                self.rolls.push(RollEntry {
                    label: format!("{num}d{sides}"),
                    details: kept,
                });
            }
        }

        Ok(self.total())
    }
}

// =========================================================================
//  Tests
// =========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    // ----- Mock dice roller -----------------------------------------------
    /// A test double: each call to roll_dice pops the next pre-loaded result.
    struct MockDiceRoller {
        results: Vec<Vec<i64>>,
        call_index: usize,
        calls: Vec<(usize, usize)>,
    }

    impl MockDiceRoller {
        fn new(results: Vec<Vec<i64>>) -> Self {
            Self {
                results,
                call_index: 0,
                calls: Vec::new(),
            }
        }

        /// Shorthand when every roll() call only needs one batch of dice.
        fn single(result: Vec<i64>) -> Self {
            Self::new(vec![result])
        }
    }

    impl DiceRoller for MockDiceRoller {
        fn roll_dice(&mut self, num: usize, sides: usize) -> Vec<i64> {
            self.calls.push((num, sides));
            let idx = self.call_index;
            self.call_index += 1;
            self.results
                .get(idx)
                .cloned()
                .unwrap_or_else(|| vec![1; num])
        }
    }

    // ===================================================================
    //  get_roll_type
    // ===================================================================
    #[test]
    fn roll_type_positive_implicit() {
        assert_eq!(get_roll_type("2d6"), ('+', "2d6"));
    }

    #[test]
    fn roll_type_positive_explicit() {
        assert_eq!(get_roll_type("+2d6"), ('+', "2d6"));
    }

    #[test]
    fn roll_type_negative() {
        assert_eq!(get_roll_type("-1d4"), ('-', "1d4"));
    }

    // ===================================================================
    //  get_keep_meta
    // ===================================================================
    #[test]
    fn keep_meta_no_keep() {
        assert_eq!(get_keep_meta("2d6"), (true, 0, "2d6"));
    }

    #[test]
    fn keep_meta_keep_high_3() {
        assert_eq!(get_keep_meta("4d6kh3"), (true, 3, "4d6"));
    }

    #[test]
    fn keep_meta_keep_low_1() {
        assert_eq!(get_keep_meta("2d20kl1"), (false, 1, "2d20"));
    }

    #[test]
    fn keep_meta_keep_high_default_num() {
        let (kh, kn, code) = get_keep_meta("2d20kh");
        assert!(kh);
        assert_eq!(kn, 1);
        assert_eq!(code, "2d20");
    }

    // ===================================================================
    //  get_num_and_sides
    // ===================================================================
    #[test]
    fn num_and_sides_explicit() {
        assert_eq!(get_num_and_sides("3d8"), (3, 8));
    }

    #[test]
    fn num_and_sides_implicit_one() {
        assert_eq!(get_num_and_sides("d20"), (1, 20));
    }

    #[test]
    fn num_and_sides_large() {
        assert_eq!(get_num_and_sides("10d100"), (10, 100));
    }

    // ===================================================================
    //  parse_code_static
    // ===================================================================
    #[test]
    fn static_positive_explicit() {
        assert_eq!(parse_code_static("+5").unwrap(), 5);
    }

    #[test]
    fn static_negative() {
        assert_eq!(parse_code_static("-3").unwrap(), -3);
    }

    #[test]
    fn static_bare_number() {
        assert_eq!(parse_code_static("7").unwrap(), 7);
    }

    #[test]
    fn static_rejects_die_code() {
        assert!(parse_code_static("2d6").is_err());
    }

    // ===================================================================
    //  find_matches (regex)
    // ===================================================================
    #[test]
    fn regex_simple() {
        assert_eq!(find_matches("1d6"), vec!["1d6"]);
    }

    #[test]
    fn regex_multiple_groups() {
        assert_eq!(find_matches("2d6+1d4"), vec!["2d6", "+1d4"]);
    }

    #[test]
    fn regex_dice_with_modifier() {
        assert_eq!(find_matches("1d20+5"), vec!["1d20", "+5"]);
    }

    #[test]
    fn regex_keep_high() {
        assert_eq!(find_matches("4d6kh3"), vec!["4d6kh3"]);
    }

    #[test]
    fn regex_keep_low() {
        assert_eq!(find_matches("2d20kl1"), vec!["2d20kl1"]);
    }

    #[test]
    fn regex_negative_modifier() {
        assert_eq!(find_matches("1d8-2"), vec!["1d8", "-2"]);
    }

    #[test]
    fn regex_implicit_one_die() {
        let m = find_matches("d20");
        assert!(m.contains(&"d20".to_string()));
    }

    // ===================================================================
    //  Roll::clear
    // ===================================================================
    #[test]
    fn clear_resets_state() {
        let mut r = Roll {
            code: "1d6".into(),
            rolls: vec![RollEntry {
                label: "1d6".into(),
                details: vec![4],
            }],
            total_static: 10,
        };
        r.clear();
        assert!(r.rolls.is_empty());
        assert_eq!(r.total_static, 0);
    }

    // ===================================================================
    //  Roll::total
    // ===================================================================
    #[test]
    fn total_sums_details_and_static() {
        let r = Roll {
            code: String::new(),
            rolls: vec![
                RollEntry { label: "2d6".into(), details: vec![5, 3] },
                RollEntry { label: "1d4".into(), details: vec![2] },
            ],
            total_static: 4,
        };
        assert_eq!(r.total(), 14); // 5+3+2+4
    }

    #[test]
    fn total_empty() {
        let r = Roll { code: String::new(), rolls: vec![], total_static: 0 };
        assert_eq!(r.total(), 0);
    }

    #[test]
    fn total_static_only() {
        let r = Roll { code: String::new(), rolls: vec![], total_static: -2 };
        assert_eq!(r.total(), -2);
    }

    // ===================================================================
    //  MockDiceRoller sanity
    // ===================================================================
    #[test]
    fn mock_roller_returns_loaded_results() {
        let mut m = MockDiceRoller::single(vec![4, 4, 4]);
        assert_eq!(m.roll_dice(3, 6), vec![4, 4, 4]);
        assert_eq!(m.calls, vec![(3, 6)]);
    }

    #[test]
    fn mock_roller_records_calls() {
        let mut m = MockDiceRoller::new(vec![vec![1], vec![5, 3]]);
        m.roll_dice(1, 20);
        m.roll_dice(2, 6);
        assert_eq!(m.calls, vec![(1, 20), (2, 6)]);
    }

    // ===================================================================
    //  Roll::new / Roll::roll  (mock-injected)
    // ===================================================================
    #[test]
    fn simple_1d6() {
        let mut m = MockDiceRoller::single(vec![4]);
        let r = Roll::new("1d6", &mut m).unwrap();
        assert_eq!(r.total(), 4);
        assert_eq!(m.calls, vec![(1, 6)]);
    }

    #[test]
    fn implicit_d20() {
        let mut m = MockDiceRoller::single(vec![17]);
        let r = Roll::new("d20", &mut m).unwrap();
        assert_eq!(r.total(), 17);
    }

    #[test]
    fn static_modifier_positive() {
        let mut m = MockDiceRoller::single(vec![3]);
        let r = Roll::new("1d6+2", &mut m).unwrap();
        assert_eq!(r.total_static, 2);
        assert_eq!(r.total(), 5);
    }

    #[test]
    fn static_modifier_negative() {
        let mut m = MockDiceRoller::single(vec![6]);
        let r = Roll::new("1d6-1", &mut m).unwrap();
        assert_eq!(r.total_static, -1);
        assert_eq!(r.total(), 5);
    }

    #[test]
    fn keep_high_3_of_4d6() {
        let mut m = MockDiceRoller::single(vec![2, 5, 3, 6]);
        let r = Roll::new("4d6kh3", &mut m).unwrap();
        // sorted desc: [6,5,3,2] → kept: [6,5,3]
        assert_eq!(r.rolls.len(), 1);
        assert_eq!(r.rolls[0].details, vec![6, 5, 3]);
        assert_eq!(r.total(), 14);
    }

    #[test]
    fn keep_low_1_of_2d20() {
        let mut m = MockDiceRoller::single(vec![14, 7]);
        let r = Roll::new("2d20kl1", &mut m).unwrap();
        // sorted asc: [7, 14] → kept: [7]
        assert_eq!(r.rolls.len(), 1);
        assert_eq!(r.rolls[0].details, vec![7]);
        assert_eq!(r.total(), 7);
    }

    #[test]
    fn two_dice_groups() {
        let mut m = MockDiceRoller::new(vec![vec![4], vec![3]]);
        let r = Roll::new("1d6+1d4", &mut m).unwrap();
        assert_eq!(r.total(), 7);
        assert_eq!(m.calls, vec![(1, 6), (1, 4)]);
    }

    #[test]
    fn reroll_clears_previous() {
        let mut m = MockDiceRoller::new(vec![vec![4], vec![6]]);
        let mut r = Roll::new("1d6", &mut m).unwrap();
        assert_eq!(r.total(), 4);

        // Second batch is consumed on reroll
        r.roll(&mut m).unwrap();
        assert_eq!(r.total(), 6);
    }

    #[test]
    fn invalid_code_returns_error() {
        let mut m = MockDiceRoller::single(vec![]);
        let result = Roll::new("zzz", &mut m);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RollError::InvalidDieCode(_)));
    }

    // ===================================================================
    //  Init / construction checks
    // ===================================================================
    #[test]
    fn new_stores_code() {
        let mut m = MockDiceRoller::single(vec![10]);
        let r = Roll::new("1d20", &mut m).unwrap();
        assert_eq!(r.code, "1d20");
    }

    #[test]
    fn new_populates_rolls() {
        let mut m = MockDiceRoller::single(vec![3]);
        let r = Roll::new("1d8", &mut m).unwrap();
        assert!(!r.rolls.is_empty());
    }

    // ===================================================================
    //  Edge cases
    // ===================================================================
    #[test]
    fn large_dice_expression() {
        // 10d10+5
        let mut m = MockDiceRoller::single(vec![5; 10]);
        let r = Roll::new("10d10+5", &mut m).unwrap();
        // no k → keep all 10 dice (each = 5), plus static +5
        assert_eq!(r.total(), 55); // (5 * 10) + 5
    }

    #[test]
    fn multiple_static_modifiers() {
        let mut m = MockDiceRoller::single(vec![10]);
        let r = Roll::new("1d20+2-1", &mut m).unwrap();
        assert_eq!(r.total_static, 1); // +2 + -1
        assert_eq!(r.total(), 11);
    }

    // ===================================================================
    //  Compound expressions (mixed dice groups + statics + keep)
    // ===================================================================

    #[test]
    fn compound_1d20_plus5_plus3d6kh1() {
        // "1d20+5+3d6kh1"
        // Tokens: ["1d20", "+5", "+3d6kh1"]
        //   1d20  → mock returns [14], no keep → kept [14]
        //   +5    → static
        //   3d6kh1 → mock returns [2, 5, 3], sorted desc [5,3,2], keep 1 → [5]
        // total = 14 + 5 + 5 = 24
        let mut m = MockDiceRoller::new(vec![vec![14], vec![2, 5, 3]]);
        let r = Roll::new("1d20+5+3d6kh1", &mut m).unwrap();

        assert_eq!(m.calls, vec![(1, 20), (3, 6)]);
        assert_eq!(r.total_static, 5);
        assert_eq!(r.rolls.len(), 2);
        assert_eq!(r.rolls[0].details, vec![14]);
        assert_eq!(r.rolls[1].details, vec![5]);
        assert_eq!(r.total(), 24);
    }

    #[test]
    fn compound_regex_parses_all_tokens() {
        let tokens = find_matches("1d20+5+3d6kh1");
        assert_eq!(tokens, vec!["1d20", "+5", "+3d6kh1"]);
    }

    #[test]
    fn compound_2d8_minus2_plus4d6kh3() {
        // "2d8-2+4d6kh3"
        //   2d8   → mock [7, 3], no k → keep all, sorted desc [7,3]
        //   -2    → static
        //   4d6kh3 → mock [1, 6, 4, 2], sorted desc [6,4,2,1], keep 3 → [6,4,2]
        // total = (7+3) + (-2) + (6+4+2) = 20
        let mut m = MockDiceRoller::new(vec![vec![7, 3], vec![1, 6, 4, 2]]);
        let r = Roll::new("2d8-2+4d6kh3", &mut m).unwrap();

        assert_eq!(m.calls, vec![(2, 8), (4, 6)]);
        assert_eq!(r.total_static, -2);
        assert_eq!(r.rolls[0].details, vec![7, 3]);
        assert_eq!(r.rolls[1].details, vec![6, 4, 2]);
        assert_eq!(r.total(), 20);
    }

    #[test]
    fn compound_d20_plus_d8_plus5_minus1() {
        // "d20+d8+5-1"
        //   d20 → [18], no k, keep all (1 die) → [18]
        //   d8  → [6], no k, keep all (1 die) → [6]
        //   +5  → static
        //   -1  → static
        // total = 18 + 6 + 5 - 1 = 28
        let mut m = MockDiceRoller::new(vec![vec![18], vec![6]]);
        let r = Roll::new("d20+d8+5-1", &mut m).unwrap();

        assert_eq!(r.total_static, 4);
        assert_eq!(r.rolls.len(), 2);
        assert_eq!(r.total(), 28);
    }

    #[test]
    fn compound_2d20kl1_plus4d6kh3_minus3() {
        // "2d20kl1+4d6kh3-3"  (disadvantage attack + ability score roll - penalty)
        //   2d20kl1 → [17, 4], sorted asc [4,17], keep 1 → [4]
        //   4d6kh3  → [3, 5, 6, 1], sorted desc [6,5,3,1], keep 3 → [6,5,3]
        //   -3      → static
        // total = 4 + 6+5+3 + (-3) = 15
        let mut m = MockDiceRoller::new(vec![vec![17, 4], vec![3, 5, 6, 1]]);
        let r = Roll::new("2d20kl1+4d6kh3-3", &mut m).unwrap();

        assert_eq!(m.calls, vec![(2, 20), (4, 6)]);
        assert_eq!(r.total_static, -3);
        assert_eq!(r.rolls[0].details, vec![4]);
        assert_eq!(r.rolls[1].details, vec![6, 5, 3]);
        assert_eq!(r.total(), 15);
    }

    #[test]
    fn compound_subtraction_code() {
        // "1d20+5-2d6"  (attack roll + static modifier - some outside effect)
        //   1d20 → [15], no k, keep all (1 die) → [15]
        //   +5   → static
        //   -2d6 → sign='-', [4, 3], no k, keep all, sorted desc [4,3]
        //          sign applied: [-4, -3]
        // total = 15 + 5 + (-4) + (-3) = 13
        let mut m = MockDiceRoller::new(vec![vec![15], vec![4, 3]]);
        let r = Roll::new("1d20+5-2d6", &mut m).unwrap();

        assert_eq!(m.calls, vec![(1, 20), (2, 6)]);
        assert_eq!(r.total_static, 5);
        assert_eq!(r.rolls[0].details, vec![15]);
        assert_eq!(r.rolls[1].details, vec![-4, -3]);
        assert_eq!(r.total(), 13); // 15 + 5 - 4 - 3
    }

    #[test]
    fn compound_reroll_preserves_code() {
        // Roll once, reroll, make sure the same compound code is re-evaluated
        let mut m = MockDiceRoller::new(vec![
            vec![10], vec![2, 5, 3],   // first roll()
            vec![20], vec![6, 6, 1],   // second roll()
        ]);
        let mut r = Roll::new("1d20+5+3d6kh1", &mut m).unwrap();
        assert_eq!(r.total(), 20); // 10 + 5 + 5

        r.roll(&mut m).unwrap();
        assert_eq!(r.total(), 31); // 20 + 5 + 6
        assert_eq!(r.code, "1d20+5+3d6kh1");
    }
}

// =========================================================================
//  PyO3 Python bindings
// =========================================================================
mod python {
    use pyo3::prelude::*;
    use pyo3::exceptions::PyValueError;
    use super::*;

    /// A single dice-group result exposed to Python.
    #[pyclass(name = "RollEntry")]
    #[derive(Clone)]
    pub struct PyRollEntry {
        #[pyo3(get)]
        pub label: String,
        #[pyo3(get)]
        pub details: Vec<i64>,
    }

    /// Python-facing dice roller.
    ///
    /// >>> from roller import Roll
    /// >>> r = Roll("4d6kh3")
    /// >>> r.total          # int
    /// >>> r.total_static   # int
    /// >>> r.rolls          # list[RollEntry]
    /// >>> r.code           # str
    /// >>> r.reroll()       # re-evaluate with fresh randomness
    #[pyclass(name = "Roll")]
    pub struct PyRoll {
        inner: Roll,
    }

    #[pymethods]
    impl PyRoll {
        #[new]
        fn new(die_code: &str) -> PyResult<Self> {
            Roll::new_random(die_code)
                .map(|inner| PyRoll { inner })
                .map_err(|e| PyValueError::new_err(e.to_string()))
        }

        /// The die code string this Roll was created with.
        #[getter]
        fn code(&self) -> &str {
            &self.inner.code
        }

        /// Sum of all kept dice plus static modifiers.
        #[getter]
        fn total(&self) -> i64 {
            self.inner.total()
        }

        /// Accumulated static modifier value.
        #[getter]
        fn total_static(&self) -> i64 {
            self.inner.total_static
        }

        /// Per-group roll results as a list of RollEntry objects.
        #[getter]
        fn rolls(&self) -> Vec<PyRollEntry> {
            self.inner
                .rolls
                .iter()
                .map(|e| PyRollEntry {
                    label: e.label.clone(),
                    details: e.details.clone(),
                })
                .collect()
        }

        /// Re-evaluate the roll with fresh randomness.
        fn reroll(&mut self) -> PyResult<i64> {
            self.inner
                .roll(&mut RandomDiceRoller)
                .map_err(|e| PyValueError::new_err(e.to_string()))
        }

        fn __repr__(&self) -> String {
            format!(
                "Roll('{}', total={}, static={}, groups={})",
                self.inner.code,
                self.inner.total(),
                self.inner.total_static,
                self.inner.rolls.len()
            )
        }
    }

    /// The Python module definition. Importable as `import roller`.
    #[pymodule]
    fn roller(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_class::<PyRoll>()?;
        m.add_class::<PyRollEntry>()?;
        Ok(())
    }
}
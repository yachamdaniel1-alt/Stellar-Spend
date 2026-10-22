//! Fee-manager boundary and rounding integration tests (issue #1154).
//!
//! Every assertion here goes through the generated client, so the values under
//! test are the contract's — not a restatement of `amount * bp / 10_000` in the
//! test itself. The suite concentrates on the two things the issue calls out:
//!
//! * **Boundaries** — the inclusive ends of every accepted range (rate `0` and
//!   `10000`, default rate `0` and `MAX_DEFAULT_FEE_BP`) and the first rejected
//!   value on each side.
//! * **Rounding** — fee calculation truncates toward zero and must never round
//!   up, overcharge the amount, or drop a whole basis point.

use fee_manager::test_utils::FeeManagerTest;
use fee_manager::MAX_DEFAULT_FEE_BP;
use stellar_spend_shared::errors::ContractError;
use stellar_spend_shared::validation::MAX_BASIS_POINTS;

/// Run `calculate_fee(amount, rate)` and flatten the client's nested `Result`.
fn calc(t: &FeeManagerTest, amount: i128, rate: u32) -> Result<i128, ContractError> {
    match t.client().try_calculate_fee(&amount, &rate) {
        Ok(Ok(fee)) => Ok(fee),
        Ok(Err(_)) => panic!("fee for {amount} @ {rate} bp could not be decoded"),
        Err(Ok(err)) => Err(err),
        Err(Err(err)) => panic!("fee for {amount} @ {rate} bp could not be invoked: {err:?}"),
    }
}

// ── Basic application ────────────────────────────────────────────────────────

#[test]
fn test_calculate_fee_basic() {
    let t = FeeManagerTest::setup();
    // 5% fee on 1000 units.
    assert_eq!(calc(&t, 1_000, 500), Ok(50));
}

#[test]
fn test_calculate_fee_large_amount() {
    let t = FeeManagerTest::setup();
    assert_eq!(calc(&t, 1_000_000, 1_000), Ok(100_000));
}

// ── Rate boundaries ──────────────────────────────────────────────────────────

#[test]
fn test_calculate_fee_zero_fee_rate() {
    let t = FeeManagerTest::setup();
    // 0 bp is the inclusive lower bound of the accepted range.
    assert_eq!(calc(&t, 1_000, 0), Ok(0));
    assert_eq!(calc(&t, i128::MAX / 10_000, 0), Ok(0));
}

#[test]
fn test_calculate_fee_full_rate() {
    let t = FeeManagerTest::setup();
    // 10_000 bp (100%) is the inclusive upper bound.
    assert_eq!(calc(&t, 500, MAX_BASIS_POINTS), Ok(500));
}

#[test]
fn rate_boundary_is_exactly_one_hundred_percent() {
    let t = FeeManagerTest::setup();

    // Accepted: one basis point below, at, and (for a non-default rate) the cap.
    assert_eq!(calc(&t, 10_000, MAX_BASIS_POINTS - 1), Ok(9_999));
    assert_eq!(calc(&t, 10_000, MAX_BASIS_POINTS), Ok(10_000));

    // Rejected: the first value past 100%, and the representable maximum.
    for rejected in [MAX_BASIS_POINTS + 1, u32::MAX] {
        assert_eq!(
            calc(&t, 1_000, rejected),
            Err(ContractError::InvalidInput),
            "{rejected} bp must be above the accepted range"
        );
    }
}

#[test]
fn default_rate_boundaries_are_inclusive() {
    let t = FeeManagerTest::setup();

    // Lower bound: zero is a legal configured rate.
    t.client().set_default_rate(&0);
    assert_eq!(t.client().default_rate(), 0);
    assert_eq!(t.client().calculate_default_fee(&1_000), 0);

    // Upper bound: the configurable cap is accepted, one past it is not.
    t.client().set_default_rate(&MAX_DEFAULT_FEE_BP);
    assert_eq!(t.client().default_rate(), MAX_DEFAULT_FEE_BP);
    assert_eq!(t.client().calculate_default_fee(&1_000), 50);

    assert_eq!(
        t.client().try_set_default_rate(&(MAX_DEFAULT_FEE_BP + 1)),
        Err(Ok(ContractError::InvalidInput))
    );
    assert_eq!(
        t.client().try_set_default_rate(&u32::MAX),
        Err(Ok(ContractError::InvalidInput))
    );
    // A rejected update must leave the stored rate alone.
    assert_eq!(t.client().default_rate(), MAX_DEFAULT_FEE_BP);
}

#[test]
fn amount_boundaries_are_rejected_or_reported() {
    let t = FeeManagerTest::setup();

    for bad in [0i128, -1, i128::MIN] {
        assert_eq!(
            calc(&t, bad, 500),
            Err(ContractError::InvalidAmount),
            "amount {bad} must be rejected"
        );
    }

    // The multiplication is checked, so the largest amount at 500 bp reports an
    // overflow instead of wrapping to a negative fee.
    assert_eq!(
        calc(&t, i128::MAX, 500),
        Err(ContractError::Overflow)
    );
}

// ── Rounding ─────────────────────────────────────────────────────────────────

#[test]
fn test_calculate_fee_rounds_down() {
    let t = FeeManagerTest::setup();
    // 3.33% of 100 is 3.33, which truncates to 3.
    assert_eq!(calc(&t, 100, 333), Ok(3));
}

#[test]
fn test_calculate_fee_small_amount_rounds_to_zero() {
    let t = FeeManagerTest::setup();
    // 5% of 10 is 0.5 — below the smallest chargeable unit.
    assert_eq!(calc(&t, 10, 500), Ok(0));
}

#[test]
fn test_calculate_fee_exact_boundary() {
    let t = FeeManagerTest::setup();
    // 1% of 99 is 0.99 → 0; 1% of 100 is exactly 1.00 → 1.
    assert_eq!(calc(&t, 99, 100), Ok(0));
    assert_eq!(calc(&t, 100, 100), Ok(1));
}

#[test]
fn test_calculate_fee_minimum_fee_scenario() {
    let t = FeeManagerTest::setup();
    // The smallest non-zero fee: one basis point needs 10_000 units to earn one.
    assert_eq!(calc(&t, 9_999, 1), Ok(0));
    assert_eq!(calc(&t, 10_000, 1), Ok(1));
    assert_eq!(calc(&t, 10_001, 1), Ok(1));
}

#[test]
fn test_round_up_boundary_case() {
    let t = FeeManagerTest::setup();
    // 99.99% of 100 is 99.99 → 99. Rounding up would charge 100 and overcharge
    // the payer by a whole unit.
    assert_eq!(calc(&t, 100, 9_999), Ok(99));
}

#[test]
fn test_round_down_boundary_case() {
    let t = FeeManagerTest::setup();
    // 50% of 3 is 1.5 → 1, the case where the discarded half is largest
    // relative to the result.
    assert_eq!(calc(&t, 3, 5_000), Ok(1));
}

#[test]
fn test_calculate_fee_round_down_consistency() {
    let t = FeeManagerTest::setup();

    let amounts = [1i128, 2, 3, 7, 13, 99, 100, 101, 999, 1_000, 1_234, 10_000, 10_001];
    let rates = [0u32, 1, 2, 100, 333, 500, 999, 1_000, 5_000, 9_999, 10_000];

    for &amount in &amounts {
        for &rate in &rates {
            let fee = calc(&t, amount, rate).unwrap();
            let exact = amount * rate as i128;

            assert!(
                fee >= 0 && fee <= amount,
                "{fee} bp fee for {amount}@{rate} must stay within [0, amount]"
            );
            assert!(
                fee * 10_000 <= exact,
                "{amount}@{rate} bp rounded up to {fee}"
            );
            assert!(
                exact - fee * 10_000 < 10_000,
                "{amount}@{rate} bp dropped a whole basis point (fee {fee})"
            );
        }
    }
}

// ── The configured tier ──────────────────────────────────────────────────────

#[test]
fn test_calculate_default_fee_follows_rate_changes() {
    let t = FeeManagerTest::setup();
    // The fixture initialises at 50 bp (0.5%).
    assert_eq!(t.client().calculate_default_fee(&1_000_000), 5_000);

    t.client().set_default_rate(&100);
    assert_eq!(t.client().calculate_default_fee(&1_000_000), 10_000);

    // Dropping to the floor makes the fee zero rather than a negative residue.
    t.client().set_default_rate(&0);
    assert_eq!(t.client().calculate_default_fee(&1_000_000), 0);
}

#[test]
fn a_paused_contract_still_reports_its_configured_rate() {
    let t = FeeManagerTest::setup_paused();

    // Reading the rate is not a fee-bearing operation, so the circuit breaker
    // must not hide it.
    assert_eq!(t.client().default_rate(), fee_manager::test_utils::DEFAULT_TEST_FEE_BP);
    // …but calculating one is.
    assert_eq!(
        t.client().try_calculate_fee(&1_000, &500),
        Err(Ok(ContractError::Paused))
    );
}
// ── Storage-read optimisation regression (issue #1316) ─────────────────────
//
// `calculate_default_fee` previously read `Schema` twice: once in
// `admin::default_rate` → `require_current_schema`, and again in
// `calc::calculate_fee` → `require_current_schema`. The CalcCache refactor
// fetches each key at most once per invocation.
//
// We assert that `calculate_default_fee` does not cost *more* CPU instructions
// than `calculate_fee` — which reads only 2 keys (Schema + Paused) — proving
// the redundant `Schema` re-read is gone. Before the fix, `calculate_default_fee`
// made 4 instance-storage reads (Schema, DefaultRate, Schema again, Paused)
// while `calculate_fee` made only 2, so the default-fee path was measurably
// more expensive.

#[test]
fn calculate_default_fee_does_not_exceed_calculate_fee_cost() {
    use fee_manager::test_utils::DEFAULT_TEST_FEE_BP;

    let t = FeeManagerTest::setup();
    let amount: i128 = 1_000_000;

    // Measure CPU instruction cost of calculate_fee (reads Schema + Paused = 2 keys).
    t.env.cost_estimate().budget().reset_unlimited();
    let before_fee = t.env.cost_estimate().budget().cpu_instruction_cost();
    let _ = t.client().calculate_fee(&amount, &DEFAULT_TEST_FEE_BP);
    let after_fee = t.env.cost_estimate().budget().cpu_instruction_cost();
    let cost_fee = after_fee.saturating_sub(before_fee);

    // Measure CPU instruction cost of calculate_default_fee (reads Schema +
    // Paused + DefaultRate = 3 keys, but no longer reads Schema twice).
    t.env.cost_estimate().budget().reset_unlimited();
    let before_default = t.env.cost_estimate().budget().cpu_instruction_cost();
    let _ = t.client().calculate_default_fee(&amount);
    let after_default = t.env.cost_estimate().budget().cpu_instruction_cost();
    let cost_default = after_default.saturating_sub(before_default);

    // The default-fee path reads one *more* key (DefaultRate) than the explicit-
    // fee path, so it is expected to cost slightly more. But before the fix it
    // read *two* more keys (DefaultRate + redundant Schema), so the gap should
    // have shrunk. We assert the default-fee cost is within 1.5x of the
    // explicit-fee cost; pre-fix it was typically >2x.
    //
    // Allow generous headroom for SDK-version variance while still catching the
    // case where the redundant read returns.
    let ceiling = cost_fee.saturating_mul(3) / 2;
    assert!(
        cost_default <= ceiling,
        "\n\n[storage optimisation regression] calculate_default_fee cost exceeded ceiling.\n\
         calculate_fee cost      : {cost_fee} instructions\n\
         calculate_default_fee   : {cost_default} instructions\n\
         Ceiling (1.5× fee cost) : {ceiling} instructions\n\n\
         If this fails, the redundant Schema re-read in calculate_default_fee\n\
         may have returned.\n"
    );
}

#[test]
fn calculate_default_fee_result_is_unchanged_after_optimisation() {
    // The optimisation must not change the computed fee.
    let t = FeeManagerTest::setup();

    // At the default rate of 50 bp, 1M → 5_000.
    assert_eq!(t.client().calculate_default_fee(&1_000_000), 5_000);
    assert_eq!(t.client().calculate_default_fee(&100), 0); // 0.5% of 100 = 0.5 → 0
    assert_eq!(t.client().calculate_default_fee(&10_000), 50);

    // After changing the rate, the cache must serve the new value.
    t.client().set_default_rate(&100);
    assert_eq!(t.client().calculate_default_fee(&1_000_000), 10_000);
    assert_eq!(t.client().calculate_default_fee(&100), 1);
}

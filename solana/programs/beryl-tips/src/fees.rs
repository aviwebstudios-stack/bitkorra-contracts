//! The protocol fee, in basis points of the amount paid. The Sepolia
//! contracts use the same rule (contracts/src/BerylFees.sol), and the app's
//! lib/beryva/creator/fees.ts the same maths.
//!
//! - Default 0. Only the config's authority can change it.
//! - Hard cap MAX_FEE_BPS (5%), checked here and in `set_fee`, so even the
//!   authority can never take more.
//! - Rounded down: the creator gets any fraction of a unit.

pub const MAX_FEE_BPS: u16 = 500;
pub const BPS_DENOMINATOR: u64 = 10_000;

/// Splits `amount` into (to the creator, to the treasury). None when the fee
/// is above the cap. The two parts always add up to `amount` exactly.
pub fn split(amount: u64, fee_bps: u16) -> Option<(u64, u64)> {
    if fee_bps > MAX_FEE_BPS {
        return None;
    }
    let fee = (amount as u128).checked_mul(fee_bps as u128)? / BPS_DENOMINATOR as u128;
    let fee = u64::try_from(fee).ok()?;
    let to_creator = amount.checked_sub(fee)?;
    Some((to_creator, fee))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_fee_pays_everything_to_the_creator() {
        assert_eq!(split(1_000_000, 0), Some((1_000_000, 0)));
    }

    #[test]
    fn fee_rounds_down() {
        // 2.5% of 999 is 24.975: the treasury gets 24, the creator 975.
        assert_eq!(split(999, 250), Some((975, 24)));
        assert_eq!(split(1, MAX_FEE_BPS), Some((1, 0)));
    }

    #[test]
    fn the_cap_holds_and_large_amounts_do_not_overflow() {
        assert_eq!(split(100, MAX_FEE_BPS + 1), None);
        assert_eq!(
            split(u64::MAX, MAX_FEE_BPS),
            Some((u64::MAX - u64::MAX / 20, u64::MAX / 20))
        );
    }
}

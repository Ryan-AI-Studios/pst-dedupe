//! Luhn checksum for credit-card post-validation.

/// Return true when `digits` (ASCII digits only) passes the Luhn check.
///
/// Empty input or non-digit characters → false.
pub fn luhn_valid(digits: &str) -> bool {
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let mut sum = 0u32;
    let mut double = false;
    for b in digits.bytes().rev() {
        let mut d = (b - b'0') as u32;
        if double {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
        double = !double;
    }
    sum.is_multiple_of(10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_test_visa_valid() {
        assert!(luhn_valid("4111111111111111"));
    }

    #[test]
    fn invalid_luhn() {
        assert!(!luhn_valid("4111111111111112"));
        assert!(!luhn_valid("1234567890123456"));
    }

    #[test]
    fn rejects_empty_and_non_digits() {
        assert!(!luhn_valid(""));
        assert!(!luhn_valid("4111-1111-1111-1111"));
    }
}

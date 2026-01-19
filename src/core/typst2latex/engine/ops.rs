//! Operations on values.
//!
//! This module implements arithmetic, comparison, and logical operations
//! for the MiniEval interpreter.

use super::value::{ContentNode, EvalError, EvalResult, Value};

/// Join two values together (concatenation).
///
/// This is used for joining content in loops and blocks.
pub fn join(lhs: Value, rhs: Value) -> EvalResult<Value> {
    use Value::*;
    Ok(match (lhs, rhs) {
        // None is identity for join
        (a, None) => a,
        (None, b) => b,

        // String concatenation
        (Str(a), Str(b)) => Str(a + &b),
        (Str(a), Int(b)) => Str(format!("{}{}", a, b)),
        (Str(a), Float(b)) => Str(format!("{}{}", a, b)),
        (Int(a), Str(b)) => Str(format!("{}{}", a, b)),
        (Float(a), Str(b)) => Str(format!("{}{}", a, b)),

        // Array concatenation
        (Array(mut a), Array(b)) => {
            a.extend(b);
            Array(a)
        }

        // Content joining
        (Content(mut a), Content(b)) => {
            a.extend(b);
            Content(a)
        }
        (Content(mut a), b) => {
            a.push(ContentNode::Text(b.display()));
            Content(a)
        }
        (a, Content(mut b)) => {
            b.insert(0, ContentNode::Text(a.display()));
            Content(b)
        }

        // Dict merging
        (Dict(mut a), Dict(b)) => {
            a.extend(b);
            Dict(a)
        }

        (a, b) => {
            return Err(EvalError::invalid_op(format!(
                "cannot join {} with {}",
                a.type_name(),
                b.type_name()
            )))
        }
    })
}

/// Apply unary plus operator.
pub fn pos(value: Value) -> EvalResult<Value> {
    use Value::*;
    match value {
        Int(v) => Ok(Int(v)),
        Float(v) => Ok(Float(v)),
        Length(l) => Ok(Length(l)),
        Ratio(r) => Ok(Ratio(r)),
        Angle(a) => Ok(Angle(a)),
        Fraction(f) => Ok(Fraction(f)),
        v => Err(EvalError::invalid_op(format!(
            "cannot apply unary '+' to {}",
            v.type_name()
        ))),
    }
}

/// Apply unary negation operator.
pub fn neg(value: Value) -> EvalResult<Value> {
    use Value::*;
    match value {
        Int(v) => v
            .checked_neg()
            .map(Int)
            .ok_or(EvalError::invalid_op("integer overflow".to_string())),
        Float(v) => Ok(Float(-v)),
        Length(l) => Ok(Length(l.negate())),
        Ratio(r) => Ok(Ratio(-r)),
        Angle(a) => Ok(Angle(-a)),
        Fraction(f) => Ok(Fraction(-f)),
        v => Err(EvalError::invalid_op(format!(
            "cannot apply unary '-' to {}",
            v.type_name()
        ))),
    }
}

/// Add two values.
pub fn add(lhs: Value, rhs: Value) -> EvalResult<Value> {
    use Value::*;
    Ok(match (lhs, rhs) {
        // Identity
        (a, None) => a,
        (None, b) => b,

        // Numeric addition
        (Int(a), Int(b)) => Int(a
            .checked_add(b)
            .ok_or(EvalError::invalid_op("integer overflow".to_string()))?),
        (Int(a), Float(b)) => Float(a as f64 + b),
        (Float(a), Int(b)) => Float(a + b as f64),
        (Float(a), Float(b)) => Float(a + b),

        // Length addition
        (Length(a), Length(b)) => Length(super::value::Length::Sum(Box::new(a), Box::new(b))),

        // Ratio addition
        (Ratio(a), Ratio(b)) => Ratio(a + b),

        // Angle addition
        (Angle(a), Angle(b)) => Angle(a + b),

        // Fraction addition
        (Fraction(a), Fraction(b)) => Fraction(a + b),

        // String concatenation
        (Str(a), Str(b)) => Str(a + &b),

        // Array concatenation
        (Array(mut a), Array(b)) => {
            a.extend(b);
            Array(a)
        }

        // Content concatenation
        (Content(mut a), Content(b)) => {
            a.extend(b);
            Content(a)
        }

        // Dict merging
        (Dict(mut a), Dict(b)) => {
            a.extend(b);
            Dict(a)
        }

        (a, b) => {
            return Err(EvalError::invalid_op(format!(
                "cannot add {} and {}",
                a.type_name(),
                b.type_name()
            )))
        }
    })
}

/// Subtract two values.
pub fn sub(lhs: Value, rhs: Value) -> EvalResult<Value> {
    use Value::*;
    match (lhs, rhs) {
        (Int(a), Int(b)) => Ok(Int(a
            .checked_sub(b)
            .ok_or(EvalError::invalid_op("integer overflow".to_string()))?)),
        (Int(a), Float(b)) => Ok(Float(a as f64 - b)),
        (Float(a), Int(b)) => Ok(Float(a - b as f64)),
        (Float(a), Float(b)) => Ok(Float(a - b)),
        (Length(a), Length(b)) => Ok(Length(super::value::Length::Sum(
            Box::new(a),
            Box::new(b.negate()),
        ))),
        (Ratio(a), Ratio(b)) => Ok(Ratio(a - b)),
        (Angle(a), Angle(b)) => Ok(Angle(a - b)),
        (Fraction(a), Fraction(b)) => Ok(Fraction(a - b)),
        (a, b) => Err(EvalError::invalid_op(format!(
            "cannot subtract {} from {}",
            b.type_name(),
            a.type_name()
        ))),
    }
}

/// Multiply two values.
pub fn mul(lhs: Value, rhs: Value) -> EvalResult<Value> {
    use Value::*;
    match (lhs, rhs) {
        (Int(a), Int(b)) => Ok(Int(a
            .checked_mul(b)
            .ok_or(EvalError::invalid_op("integer overflow".to_string()))?)),
        (Int(a), Float(b)) => Ok(Float(a as f64 * b)),
        (Float(a), Int(b)) => Ok(Float(a * b as f64)),
        (Float(a), Float(b)) => Ok(Float(a * b)),

        // Length scaling
        (Length(l), Int(n)) => Ok(Length(l.scale(n as f64))),
        (Length(l), Float(f)) => Ok(Length(l.scale(f))),
        (Int(n), Length(l)) => Ok(Length(l.scale(n as f64))),
        (Float(f), Length(l)) => Ok(Length(l.scale(f))),

        // Ratio scaling
        (Ratio(r), Int(n)) => Ok(Ratio(r * n as f64)),
        (Ratio(r), Float(f)) => Ok(Ratio(r * f)),
        (Int(n), Ratio(r)) => Ok(Ratio(r * n as f64)),
        (Float(f), Ratio(r)) => Ok(Ratio(r * f)),

        // Angle scaling
        (Angle(a), Int(n)) => Ok(Angle(a * n as f64)),
        (Angle(a), Float(f)) => Ok(Angle(a * f)),
        (Int(n), Angle(a)) => Ok(Angle(a * n as f64)),
        (Float(f), Angle(a)) => Ok(Angle(a * f)),

        // Fraction scaling
        (Fraction(fr), Int(n)) => Ok(Fraction(fr * n as f64)),
        (Fraction(fr), Float(f)) => Ok(Fraction(fr * f)),
        (Int(n), Fraction(fr)) => Ok(Fraction(fr * n as f64)),
        (Float(f), Fraction(fr)) => Ok(Fraction(fr * f)),

        // String repetition
        (Str(s), Int(n)) => {
            if n < 0 {
                return Err(EvalError::invalid_op(
                    "cannot repeat string negative times".to_string(),
                ));
            }
            Ok(Str(s.repeat(n as usize)))
        }
        (Int(n), Str(s)) => {
            if n < 0 {
                return Err(EvalError::invalid_op(
                    "cannot repeat string negative times".to_string(),
                ));
            }
            Ok(Str(s.repeat(n as usize)))
        }

        // Array repetition
        (Array(arr), Int(n)) => {
            if n < 0 {
                return Err(EvalError::invalid_op(
                    "cannot repeat array negative times".to_string(),
                ));
            }
            let mut result = Vec::new();
            for _ in 0..n {
                result.extend(arr.clone());
            }
            Ok(Array(result))
        }

        (a, b) => Err(EvalError::invalid_op(format!(
            "cannot multiply {} and {}",
            a.type_name(),
            b.type_name()
        ))),
    }
}

/// Divide two values.
pub fn div(lhs: Value, rhs: Value) -> EvalResult<Value> {
    use Value::*;
    match (lhs, rhs) {
        (Int(a), Int(b)) => {
            if b == 0 {
                return Err(EvalError::div_zero());
            }
            // Typst uses float division for integers
            Ok(Float(a as f64 / b as f64))
        }
        (Int(a), Float(b)) => {
            if b == 0.0 {
                return Err(EvalError::div_zero());
            }
            Ok(Float(a as f64 / b))
        }
        (Float(a), Int(b)) => {
            if b == 0 {
                return Err(EvalError::div_zero());
            }
            Ok(Float(a / b as f64))
        }
        (Float(a), Float(b)) => {
            if b == 0.0 {
                return Err(EvalError::div_zero());
            }
            Ok(Float(a / b))
        }
        // Length / number
        (Length(l), Int(n)) => {
            if n == 0 {
                return Err(EvalError::div_zero());
            }
            Ok(Length(l.scale(1.0 / n as f64)))
        }
        (Length(l), Float(f)) => {
            if f == 0.0 {
                return Err(EvalError::div_zero());
            }
            Ok(Length(l.scale(1.0 / f)))
        }
        // Ratio / number
        (Ratio(r), Int(n)) => {
            if n == 0 {
                return Err(EvalError::div_zero());
            }
            Ok(Ratio(r / n as f64))
        }
        (Ratio(r), Float(f)) => {
            if f == 0.0 {
                return Err(EvalError::div_zero());
            }
            Ok(Ratio(r / f))
        }
        // Angle / number
        (Angle(a), Int(n)) => {
            if n == 0 {
                return Err(EvalError::div_zero());
            }
            Ok(Angle(a / n as f64))
        }
        (Angle(a), Float(f)) => {
            if f == 0.0 {
                return Err(EvalError::div_zero());
            }
            Ok(Angle(a / f))
        }
        // Fraction / number
        (Fraction(fr), Int(n)) => {
            if n == 0 {
                return Err(EvalError::div_zero());
            }
            Ok(Fraction(fr / n as f64))
        }
        (Fraction(fr), Float(f)) => {
            if f == 0.0 {
                return Err(EvalError::div_zero());
            }
            Ok(Fraction(fr / f))
        }
        (a, b) => Err(EvalError::invalid_op(format!(
            "cannot divide {} by {}",
            a.type_name(),
            b.type_name()
        ))),
    }
}

/// Equality comparison.
pub fn eq(lhs: &Value, rhs: &Value) -> bool {
    lhs == rhs
}

/// Inequality comparison.
pub fn ne(lhs: &Value, rhs: &Value) -> bool {
    lhs != rhs
}

/// Less than comparison.
pub fn lt(lhs: &Value, rhs: &Value) -> EvalResult<bool> {
    use Value::*;
    match (lhs, rhs) {
        (Int(a), Int(b)) => Ok(a < b),
        (Int(a), Float(b)) => Ok((*a as f64) < *b),
        (Float(a), Int(b)) => Ok(*a < *b as f64),
        (Float(a), Float(b)) => Ok(a < b),
        (Str(a), Str(b)) => Ok(a < b),
        (Ratio(a), Ratio(b)) => Ok(a < b),
        (Angle(a), Angle(b)) => Ok(a < b),
        (Fraction(a), Fraction(b)) => Ok(a < b),
        (a, b) => Err(EvalError::invalid_op(format!(
            "cannot compare {} with {}",
            a.type_name(),
            b.type_name()
        ))),
    }
}

/// Less than or equal comparison.
pub fn le(lhs: &Value, rhs: &Value) -> EvalResult<bool> {
    use Value::*;
    match (lhs, rhs) {
        (Int(a), Int(b)) => Ok(a <= b),
        (Int(a), Float(b)) => Ok((*a as f64) <= *b),
        (Float(a), Int(b)) => Ok(*a <= *b as f64),
        (Float(a), Float(b)) => Ok(a <= b),
        (Str(a), Str(b)) => Ok(a <= b),
        (Ratio(a), Ratio(b)) => Ok(a <= b),
        (Angle(a), Angle(b)) => Ok(a <= b),
        (Fraction(a), Fraction(b)) => Ok(a <= b),
        (a, b) => Err(EvalError::invalid_op(format!(
            "cannot compare {} with {}",
            a.type_name(),
            b.type_name()
        ))),
    }
}

/// Greater than comparison.
pub fn gt(lhs: &Value, rhs: &Value) -> EvalResult<bool> {
    use Value::*;
    match (lhs, rhs) {
        (Int(a), Int(b)) => Ok(a > b),
        (Int(a), Float(b)) => Ok((*a as f64) > *b),
        (Float(a), Int(b)) => Ok(*a > *b as f64),
        (Float(a), Float(b)) => Ok(a > b),
        (Str(a), Str(b)) => Ok(a > b),
        (Ratio(a), Ratio(b)) => Ok(a > b),
        (Angle(a), Angle(b)) => Ok(a > b),
        (Fraction(a), Fraction(b)) => Ok(a > b),
        (a, b) => Err(EvalError::invalid_op(format!(
            "cannot compare {} with {}",
            a.type_name(),
            b.type_name()
        ))),
    }
}

/// Greater than or equal comparison.
pub fn ge(lhs: &Value, rhs: &Value) -> EvalResult<bool> {
    use Value::*;
    match (lhs, rhs) {
        (Int(a), Int(b)) => Ok(a >= b),
        (Int(a), Float(b)) => Ok((*a as f64) >= *b),
        (Float(a), Int(b)) => Ok(*a >= *b as f64),
        (Float(a), Float(b)) => Ok(a >= b),
        (Str(a), Str(b)) => Ok(a >= b),
        (Ratio(a), Ratio(b)) => Ok(a >= b),
        (Angle(a), Angle(b)) => Ok(a >= b),
        (Fraction(a), Fraction(b)) => Ok(a >= b),
        (a, b) => Err(EvalError::invalid_op(format!(
            "cannot compare {} with {}",
            a.type_name(),
            b.type_name()
        ))),
    }
}

/// Logical NOT.
pub fn not(value: &Value) -> EvalResult<Value> {
    Ok(Value::Bool(!value.as_bool()?))
}

/// Check if a value is contained in another.
pub fn contains(container: &Value, item: &Value) -> EvalResult<bool> {
    use Value::*;
    match container {
        Str(s) => {
            let needle = item.as_str()?;
            Ok(s.contains(needle))
        }
        Array(arr) => Ok(arr.contains(item)),
        Dict(dict) => {
            let key = item.as_str()?;
            Ok(dict.contains_key(key))
        }
        c => Err(EvalError::invalid_op(format!(
            "cannot check containment in {}",
            c.type_name()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::super::value::EvalErrorKind;
    use super::*;

    #[test]
    fn test_add() {
        assert_eq!(add(Value::Int(2), Value::Int(3)).unwrap(), Value::Int(5));
        assert_eq!(
            add(Value::Float(2.5), Value::Float(1.5)).unwrap(),
            Value::Float(4.0)
        );
        assert_eq!(
            add(Value::Str("hello".into()), Value::Str(" world".into())).unwrap(),
            Value::Str("hello world".into())
        );
    }

    #[test]
    fn test_sub() {
        assert_eq!(sub(Value::Int(5), Value::Int(3)).unwrap(), Value::Int(2));
        assert_eq!(
            sub(Value::Float(5.0), Value::Float(2.5)).unwrap(),
            Value::Float(2.5)
        );
    }

    #[test]
    fn test_mul() {
        assert_eq!(mul(Value::Int(3), Value::Int(4)).unwrap(), Value::Int(12));
        assert_eq!(
            mul(Value::Str("ab".into()), Value::Int(3)).unwrap(),
            Value::Str("ababab".into())
        );
    }

    #[test]
    fn test_div() {
        assert_eq!(
            div(Value::Int(10), Value::Int(4)).unwrap(),
            Value::Float(2.5)
        );
        assert!(matches!(
            div(Value::Int(1), Value::Int(0)),
            Err(e) if matches!(e.kind, EvalErrorKind::DivisionByZero)
        ));
    }

    #[test]
    fn test_comparisons() {
        assert!(lt(&Value::Int(1), &Value::Int(2)).unwrap());
        assert!(!lt(&Value::Int(2), &Value::Int(1)).unwrap());
        assert!(le(&Value::Int(2), &Value::Int(2)).unwrap());
        assert!(gt(&Value::Int(3), &Value::Int(2)).unwrap());
        assert!(ge(&Value::Int(2), &Value::Int(2)).unwrap());
    }

    #[test]
    fn test_join() {
        let a = Value::Content(vec![ContentNode::Text("hello".into())]);
        let b = Value::Content(vec![ContentNode::Text(" world".into())]);
        let result = join(a, b).unwrap();
        if let Value::Content(nodes) = result {
            assert_eq!(nodes.len(), 2);
        } else {
            panic!("expected content");
        }
    }

    #[test]
    fn test_ratio_operations() {
        assert_eq!(
            add(Value::Ratio(0.5), Value::Ratio(0.3)).unwrap(),
            Value::Ratio(0.8)
        );
        assert_eq!(
            mul(Value::Ratio(0.5), Value::Int(2)).unwrap(),
            Value::Ratio(1.0)
        );
    }

    #[test]
    fn test_angle_operations() {
        assert_eq!(
            add(Value::Angle(45.0), Value::Angle(45.0)).unwrap(),
            Value::Angle(90.0)
        );
        assert_eq!(neg(Value::Angle(90.0)).unwrap(), Value::Angle(-90.0));
    }
}

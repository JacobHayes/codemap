//! Fixture crate.
use std::fmt;

/// A shape.
pub struct Shape {
    sides: u32,
}

pub fn area(shape: &Shape) -> u32 {
    assert!(shape.sides > 0);
    info!("computing area");
    match parse(shape.sides) {
        Ok(value) => value,
        Err(error) => {
            warn!("bad {error}");
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn area_is_positive() {
        assert_eq!(area(&Shape { sides: 3 }), 3);
    }
}

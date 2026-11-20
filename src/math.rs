//! Platform-independent math. `f64::sin` and its kin call the operating
//! system's C math library, and Windows and Linux disagree in the last bit;
//! a last bit is enough to flip a tie or a threshold and move a golden.
//! These methods call the pure-Rust `libm` instead, so every platform
//! computes the same bits. The engine uses these (`x.psin()`), never the
//! std ones. Arithmetic, `sqrt`, `abs`, `floor` and the like are exact
//! everywhere and stay as they are.

pub trait Portable: Sized {
    fn psin(self) -> Self;
    fn pcos(self) -> Self;
    fn ptan(self) -> Self;
    fn pasin(self) -> Self;
    fn pacos(self) -> Self;
    fn patan(self) -> Self;
    fn patan2(self, x: Self) -> Self;
    fn phypot(self, other: Self) -> Self;
    fn pexp(self) -> Self;
    fn pln(self) -> Self;
    fn ppowf(self, n: Self) -> Self;
    fn pcbrt(self) -> Self;
    fn psin_cos(self) -> (Self, Self);
}

impl Portable for f64 {
    fn psin(self) -> f64 { libm::sin(self) }
    fn pcos(self) -> f64 { libm::cos(self) }
    fn ptan(self) -> f64 { libm::tan(self) }
    fn pasin(self) -> f64 { libm::asin(self) }
    fn pacos(self) -> f64 { libm::acos(self) }
    fn patan(self) -> f64 { libm::atan(self) }
    fn patan2(self, x: f64) -> f64 { libm::atan2(self, x) }
    fn phypot(self, other: f64) -> f64 { libm::hypot(self, other) }
    fn pexp(self) -> f64 { libm::exp(self) }
    fn pln(self) -> f64 { libm::log(self) }
    fn ppowf(self, n: f64) -> f64 { libm::pow(self, n) }
    fn pcbrt(self) -> f64 { libm::cbrt(self) }
    fn psin_cos(self) -> (f64, f64) { (libm::sin(self), libm::cos(self)) }
}

impl Portable for f32 {
    fn psin(self) -> f32 { libm::sinf(self) }
    fn pcos(self) -> f32 { libm::cosf(self) }
    fn ptan(self) -> f32 { libm::tanf(self) }
    fn pasin(self) -> f32 { libm::asinf(self) }
    fn pacos(self) -> f32 { libm::acosf(self) }
    fn patan(self) -> f32 { libm::atanf(self) }
    fn patan2(self, x: f32) -> f32 { libm::atan2f(self, x) }
    fn phypot(self, other: f32) -> f32 { libm::hypotf(self, other) }
    fn pexp(self) -> f32 { libm::expf(self) }
    fn pln(self) -> f32 { libm::logf(self) }
    fn ppowf(self, n: f32) -> f32 { libm::powf(self, n) }
    fn pcbrt(self) -> f32 { libm::cbrtf(self) }
    fn psin_cos(self) -> (f32, f32) { (libm::sinf(self), libm::cosf(self)) }
}

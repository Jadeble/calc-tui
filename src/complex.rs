//! 复数类型与运算(主分支约定)。
//!
//! 实数只是虚部为 0 的特例: 算术与函数在实参输入时尽量走 f64 原生路径,
//! 保证既有精确行为不变(如 2^10 精确 1024、1/0 = ∞、ln(e) = 1 等)。

use std::ops::{Add, Div, Mul, Neg, Sub};

/// 复数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cmplx {
    pub re: f64,
    pub im: f64,
}

impl std::fmt::Display for Cmplx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}+{}i", self.re, self.im)
    }
}

impl Cmplx {
    pub const I: Cmplx = Cmplx { re: 0.0, im: 1.0 };

    pub fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    pub fn real(re: f64) -> Self {
        Self { re, im: 0.0 }
    }

    pub fn nan() -> Self {
        Self {
            re: f64::NAN,
            im: f64::NAN,
        }
    }

    /// 虚部为 0(含 NaN 时按非实数处理)。
    pub fn is_real(&self) -> bool {
        self.im == 0.0
    }

    pub fn is_nan(&self) -> bool {
        self.re.is_nan() || self.im.is_nan()
    }

    pub fn is_finite(&self) -> bool {
        self.re.is_finite() && self.im.is_finite()
    }

    /// 模 |z|。
    pub fn abs(&self) -> f64 {
        self.re.hypot(self.im)
    }

    /// 辐角主值 (-π, π]。
    pub fn arg(&self) -> f64 {
        if self.im == 0.0 {
            // 虚部为 ±0: 实数辐角 0 或 π(避免 -0.0 使 atan2 返回 -π)
            return if self.re >= 0.0 { 0.0 } else { PI };
        }
        self.im.atan2(self.re)
    }

    pub fn conj(&self) -> Self {
        Self::new(self.re, -self.im)
    }

    /// 指数 e^z。
    pub fn exp(self) -> Self {
        let e = self.re.exp();
        Self::new(e * cos_snap(self.im), e * sin_snap(self.im))
    }

    /// 主分支对数。
    pub fn ln(self) -> Self {
        Self::new(self.abs().ln(), self.arg())
    }

    /// 主分支平方根。
    pub fn sqrt(self) -> Self {
        if self.is_real() {
            if self.re >= 0.0 {
                return Self::real(self.re.sqrt());
            }
            // 负实数: 主分支纯虚数(精确, 且 sqrt(-∞) 可表示)
            return Self::new(0.0, (-self.re).sqrt());
        }
        let r = self.abs().sqrt();
        let th = self.arg() / 2.0;
        Self::new(r * cos_snap(th), r * sin_snap(th))
    }

    /// 幂 z^w。实底实指走 f64 powf 保持精确(含负底整数幂与未定义情形)。
    pub fn pow(self, w: Self) -> Self {
        if self.is_real() && w.is_real() {
            return Self::real(self.re.powf(w.re));
        }
        (w * self.ln()).exp()
    }
}

impl Add for Cmplx {
    type Output = Cmplx;
    fn add(self, o: Cmplx) -> Cmplx {
        Cmplx::new(self.re + o.re, self.im + o.im)
    }
}

impl Sub for Cmplx {
    type Output = Cmplx;
    fn sub(self, o: Cmplx) -> Cmplx {
        Cmplx::new(self.re - o.re, self.im - o.im)
    }
}

impl Neg for Cmplx {
    type Output = Cmplx;
    fn neg(self) -> Cmplx {
        Cmplx::new(-self.re, -self.im)
    }
}

impl Mul for Cmplx {
    type Output = Cmplx;
    fn mul(self, o: Cmplx) -> Cmplx {
        Cmplx::new(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
}

impl Div for Cmplx {
    type Output = Cmplx;
    fn div(self, o: Cmplx) -> Cmplx {
        // 实数分母: 保持 f64 除零语义 (1/0 = ∞, 0/0 = NaN)
        if o.im == 0.0 {
            if self.im == 0.0 {
                return Cmplx::real(self.re / o.re);
            }
            return Cmplx::new(self.re / o.re, self.im / o.re);
        }
        let d = o.re * o.re + o.im * o.im;
        Cmplx::new(
            (self.re * o.re + self.im * o.im) / d,
            (self.im * o.re - self.re * o.im) / d,
        )
    }
}

impl Add<f64> for Cmplx {
    type Output = Cmplx;
    fn add(self, o: f64) -> Cmplx {
        Cmplx::new(self.re + o, self.im)
    }
}

impl Sub<f64> for Cmplx {
    type Output = Cmplx;
    fn sub(self, o: f64) -> Cmplx {
        Cmplx::new(self.re - o, self.im)
    }
}

impl Mul<f64> for Cmplx {
    type Output = Cmplx;
    fn mul(self, o: f64) -> Cmplx {
        Cmplx::new(self.re * o, self.im * o)
    }
}

impl Mul<Cmplx> for f64 {
    type Output = Cmplx;
    fn mul(self, o: Cmplx) -> Cmplx {
        Cmplx::new(self * o.re, self * o.im)
    }
}

impl Div<f64> for Cmplx {
    type Output = Cmplx;
    fn div(self, o: f64) -> Cmplx {
        Cmplx::new(self.re / o, self.im / o)
    }
}

// ---------------------------------------------------------------- 精确矫正

use std::f64::consts::PI;

const HALF_PI: f64 = PI / 2.0;

/// 若 x 落在 π/2 的整数倍附近(浮点舍入误差量级), 返回 (该倍数, 偏差)。
pub(crate) fn trig_snap_arg(x: f64) -> Option<(i64, f64)> {
    let k = (x / HALF_PI).round();
    let d = x - k * HALF_PI;
    (d.abs() < 1e-12).then_some((k as i64, d))
}

/// sin 精确值矫正: sin(k·π/2) = 0/1/-1, 消除 sin(π)≈1.22e-16 这类舍入误差。
pub fn sin_snap(x: f64) -> f64 {
    if let Some((k, _)) = trig_snap_arg(x) {
        return match k.rem_euclid(4) {
            0 | 2 => 0.0,
            1 => 1.0,
            _ => -1.0,
        };
    }
    x.sin()
}

/// cos 精确值矫正: cos(k·π/2) = 1/0/-1, 消除 cos(π/2)≈6.12e-17 这类舍入误差。
pub fn cos_snap(x: f64) -> f64 {
    if let Some((k, _)) = trig_snap_arg(x) {
        return match k.rem_euclid(4) {
            0 => 1.0,
            2 => -1.0,
            _ => 0.0,
        };
    }
    x.cos()
}

/// tan 精确值矫正: tan(k·π) = 0, tan(奇数·π/2) = ±∞。
/// 趋近方向由偏差 d 决定(左侧趋近 +∞, 右侧趋近 -∞); 恰好在奇倍 π/2 上时,
/// 符号取 sign_ref 原生计算值的符号, 使 tan(-π/2) = -∞、tan(π/2) = +∞,
/// 且 DEG 与 RAD 模式对同一角度结果一致。
pub fn tan_snap_at(x: f64, sign_ref: f64) -> f64 {
    if let Some((k, d)) = trig_snap_arg(x) {
        if k.rem_euclid(2) == 0 {
            return 0.0;
        }
        return if d < 0.0 {
            f64::INFINITY
        } else if d > 0.0 {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY.copysign(sign_ref.tan())
        };
    }
    sign_ref.tan()
}

pub fn tan_snap(x: f64) -> f64 {
    tan_snap_at(x, x)
}

// ---------------------------------------------------------------- 复函数

/// sin(z) = sin(a)·cosh(b) + i·cos(a)·sinh(b)。实参走 f64 精确路径。
pub fn csin(z: Cmplx) -> Cmplx {
    if z.is_real() {
        return Cmplx::real(sin_snap(z.re));
    }
    Cmplx::new(sin_snap(z.re) * z.im.cosh(), cos_snap(z.re) * z.im.sinh())
}

pub fn ccos(z: Cmplx) -> Cmplx {
    if z.is_real() {
        return Cmplx::real(cos_snap(z.re));
    }
    Cmplx::new(cos_snap(z.re) * z.im.cosh(), -sin_snap(z.re) * z.im.sinh())
}

/// tan(z) = sin(2a)/(cosh(2b)+cos(2a)) + i·sinh(2b)/(cosh(2b)+cos(2a))。
pub fn ctan(z: Cmplx) -> Cmplx {
    if z.is_real() {
        return Cmplx::real(tan_snap(z.re));
    }
    let d = (2.0 * z.im).cosh() + cos_snap(2.0 * z.re);
    Cmplx::new(sin_snap(2.0 * z.re) / d, (2.0 * z.im).sinh() / d)
}

/// asin(z) = -i·ln(iz + sqrt(1-z²))。实参 |x|≤1 走 f64 精确路径。
pub fn casin(z: Cmplx) -> Cmplx {
    if z.is_real() && z.re.abs() <= 1.0 {
        return Cmplx::real(z.re.asin());
    }
    let one = Cmplx::real(1.0);
    ((Cmplx::I * z + (one - z * z).sqrt()).ln()) * -Cmplx::I
}

/// acos(z) = -i·ln(z + sqrt(z²-1))。
/// 注意不能用 π/2 - asin(z): 该恒等式在复数主分支下不成立。
pub fn cacos(z: Cmplx) -> Cmplx {
    if z.is_real() && z.re.abs() <= 1.0 {
        return Cmplx::real(z.re.acos());
    }
    let one = Cmplx::real(1.0);
    (z + (z * z - one).sqrt()).ln() * -Cmplx::I
}

/// atan(z) = (i/2)·(ln(1-iz) - ln(1+iz))。实参走 f64 精确路径。
pub fn catan(z: Cmplx) -> Cmplx {
    if z.is_real() {
        return Cmplx::real(z.re.atan());
    }
    let one = Cmplx::real(1.0);
    ((one - Cmplx::I * z).ln() - (one + Cmplx::I * z).ln()) * (Cmplx::I * 0.5)
}

/// sinh(z) = sinh(a)·cos(b) + i·cosh(a)·sin(b)。
pub fn csinh(z: Cmplx) -> Cmplx {
    if z.is_real() {
        return Cmplx::real(z.re.sinh());
    }
    Cmplx::new(z.re.sinh() * cos_snap(z.im), z.re.cosh() * sin_snap(z.im))
}

/// cosh(z) = cosh(a)·cos(b) + i·sinh(a)·sin(b)。
pub fn ccosh(z: Cmplx) -> Cmplx {
    if z.is_real() {
        return Cmplx::real(z.re.cosh());
    }
    Cmplx::new(z.re.cosh() * cos_snap(z.im), z.re.sinh() * sin_snap(z.im))
}

/// tanh(z) = (sinh(2a) + i·sin(2b)) / (cosh(2a) + cos(2b))。
pub fn ctanh(z: Cmplx) -> Cmplx {
    if z.is_real() {
        return Cmplx::real(z.re.tanh());
    }
    let d = (2.0 * z.re).cosh() + cos_snap(2.0 * z.im);
    Cmplx::new((2.0 * z.re).sinh() / d, sin_snap(2.0 * z.im) / d)
}

/// asinh(z) = ln(z + sqrt(z²+1))。实参走 f64 精确路径。
pub fn casinh(z: Cmplx) -> Cmplx {
    if z.is_real() {
        return Cmplx::real(z.re.asinh());
    }
    (z + (z * z + Cmplx::real(1.0)).sqrt()).ln()
}

/// acosh(z) = ln(z + sqrt(z-1)·sqrt(z+1))。实参 x≥1 走 f64 精确路径。
pub fn cacosh(z: Cmplx) -> Cmplx {
    if z.is_real() && z.re >= 1.0 {
        return Cmplx::real(z.re.acosh());
    }
    let one = Cmplx::real(1.0);
    (z + (z - one).sqrt() * (z + one).sqrt()).ln()
}

/// atanh(z) = (1/2)·(ln(1+z) - ln(1-z))。实参 |x|<1 走 f64 精确路径。
pub fn catanh(z: Cmplx) -> Cmplx {
    if z.is_real() && z.re.abs() < 1.0 {
        return Cmplx::real(z.re.atanh());
    }
    let one = Cmplx::real(1.0);
    ((one + z).ln() - (one - z).ln()) * 0.5
}

//! 轻量级表达式分词与求值引擎(仓库内实现, 替代已停维护的 meval)。
//!
//! 语法: 数字(含科学计数法)、标识符(变量/函数)、括号、逗号、
//! 二元运算符 + - * / % ^(幂, 右结合)、一元 + -。
//!
//! 优先级(与 meval 一致): ^ > 一元 > * / % > + -; 一元运算符入栈后不弹栈,
//! 因此 -2^2 = -(2^2) = -4, 2^-2 = 2^(-2) = 0.25。
//!
//! 实现流程: 分词 → 调度场算法转 RPN → 栈求值。

use crate::complex::Cmplx;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Num(f64),
    Var(String),
    Func(String, usize),
    Bin(Op),
    Un(Op),
    LParen,
    RParen,
    Comma,
}

/// 函数类型: 接收全部参数, 返回结果或错误。
pub type EvalFn = Rc<dyn Fn(&[Cmplx]) -> Result<Cmplx, String>>;

/// 求值上下文: 函数表 + 变量表。
#[derive(Default)]
pub struct ExprContext {
    funcs: HashMap<String, EvalFn>,
    vars: HashMap<String, Cmplx>,
}

impl ExprContext {
    /// 内置常量与函数(全部复数版; 实参输入时内部走 f64 精确路径)。
    pub fn new() -> Self {
        use crate::complex::{
            cacos, cacosh, casin, casinh, catan, catanh, ccos, ccosh, csin, csinh, ctan, ctanh,
        };
        let mut ctx = Self::default();
        ctx.var("pi", Cmplx::real(std::f64::consts::PI));
        ctx.var("e", Cmplx::real(std::f64::consts::E));
        ctx.var("i", Cmplx::I);
        ctx.func("sqrt", Cmplx::sqrt);
        ctx.func("exp", Cmplx::exp);
        ctx.func("ln", Cmplx::ln);
        ctx.func("abs", |z: Cmplx| Cmplx::real(z.abs()));
        ctx.func("conj", |z: Cmplx| z.conj());
        ctx.func("arg", |z: Cmplx| Cmplx::real(z.arg()));
        ctx.func("re", |z: Cmplx| Cmplx::real(z.re));
        ctx.func("im", |z: Cmplx| Cmplx::real(z.im));
        ctx.func("sin", csin);
        ctx.func("cos", ccos);
        ctx.func("tan", ctan);
        ctx.func("asin", casin);
        ctx.func("acos", cacos);
        ctx.func("atan", catan);
        ctx.func("sinh", csinh);
        ctx.func("cosh", ccosh);
        ctx.func("tanh", ctanh);
        ctx.func("asinh", casinh);
        ctx.func("acosh", cacosh);
        ctx.func("atanh", catanh);
        ctx.func_real("floor", f64::floor);
        ctx.func_real("ceil", f64::ceil);
        ctx.func_real("round", f64::round);
        ctx.func_real("signum", f64::signum);
        ctx.func2_real("atan2", f64::atan2);
        ctx.variadic_real("max", 1, |a| {
            a.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        });
        ctx.variadic_real("min", 1, |a| {
            a.iter().copied().fold(f64::INFINITY, f64::min)
        });
        ctx
    }

    /// 注册一元复函数。
    pub fn func<F>(&mut self, name: &str, f: F) -> &mut Self
    where
        F: Fn(Cmplx) -> Cmplx + 'static,
    {
        let n = name.to_string();
        self.funcs.insert(
            name.to_string(),
            Rc::new(move |a: &[Cmplx]| match a {
                [x] => Ok(f(*x)),
                _ => Err(format!("{n} 需要 1 个参数")),
            }),
        );
        self
    }

    /// 注册二元复函数。
    pub fn func2<F>(&mut self, name: &str, f: F) -> &mut Self
    where
        F: Fn(Cmplx, Cmplx) -> Cmplx + 'static,
    {
        let n = name.to_string();
        self.funcs.insert(
            name.to_string(),
            Rc::new(move |a: &[Cmplx]| match a {
                [x, y] => Ok(f(*x, *y)),
                _ => Err(format!("{n} 需要 2 个参数")),
            }),
        );
        self
    }

    /// 注册一元实数函数(虚部非零时报错)。
    pub fn func_real<F>(&mut self, name: &str, f: F) -> &mut Self
    where
        F: Fn(f64) -> f64 + 'static,
    {
        let n = name.to_string();
        self.funcs.insert(
            name.to_string(),
            Rc::new(move |a: &[Cmplx]| match a {
                [x] if x.is_real() => Ok(Cmplx::real(f(x.re))),
                [x] => Err(format!("{n} 仅支持实数 (虚部 {})", x.im)),
                _ => Err(format!("{n} 需要 1 个参数")),
            }),
        );
        self
    }

    /// 注册二元实数函数(虚部非零时报错)。
    pub fn func2_real<F>(&mut self, name: &str, f: F) -> &mut Self
    where
        F: Fn(f64, f64) -> f64 + 'static,
    {
        let n = name.to_string();
        self.funcs.insert(
            name.to_string(),
            Rc::new(move |a: &[Cmplx]| match a {
                [x, y] if x.is_real() && y.is_real() => Ok(Cmplx::real(f(x.re, y.re))),
                _ => Err(format!("{n} 仅支持实数")),
            }),
        );
        self
    }

    /// 注册变参实数函数(虚部非零时报错)。
    pub fn variadic_real<F>(&mut self, name: &str, min: usize, f: F) -> &mut Self
    where
        F: Fn(&[f64]) -> f64 + 'static,
    {
        let n = name.to_string();
        self.funcs.insert(
            name.to_string(),
            Rc::new(move |a: &[Cmplx]| {
                if a.len() < min {
                    return Err(format!("{n} 至少需要 {min} 个参数"));
                }
                if a.iter().any(|z| !z.is_real()) {
                    return Err(format!("{n} 仅支持实数"));
                }
                let rs: Vec<f64> = a.iter().map(|z| z.re).collect();
                Ok(Cmplx::real(f(&rs)))
            }),
        );
        self
    }

    /// 注册变量/常量。
    pub fn var(&mut self, name: &str, v: Cmplx) -> &mut Self {
        self.vars.insert(name.to_string(), v);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParenKind {
    Subexpr,
    Func,
}

fn is_ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// 解析一个数字: 数字[.数字][e|E[+|-]数字], 返回 (值, 结束下标)。
fn parse_number(b: &[u8], i: usize) -> Result<(f64, usize), String> {
    let mut j = i;
    while j < b.len() && b[j].is_ascii_digit() {
        j += 1;
    }
    if j < b.len() && b[j] == b'.' {
        j += 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
    }
    if j < b.len() && (b[j] == b'e' || b[j] == b'E') {
        let mut k = j + 1;
        if k < b.len() && (b[k] == b'+' || b[k] == b'-') {
            k += 1;
        }
        let dstart = k;
        while k < b.len() && b[k].is_ascii_digit() {
            k += 1;
        }
        if k > dstart {
            j = k;
        }
    }
    let text = std::str::from_utf8(&b[i..j]).map_err(|_| "数字解析失败".to_string())?;
    let v: f64 = text.parse().map_err(|_| format!("数字格式错误: {text}"))?;
    Ok((v, j))
}

/// 分词: 只接受良构表达式, 状态机区分一元/二元运算符、函数参数逗号等。
fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut toks = Vec::new();
    let mut parens: Vec<ParenKind> = Vec::new();
    // true = 期望操作数(数字/变量/函数/一元/左括号), false = 期望运算符/右括号/逗号
    let mut expect_operand = true;
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let t = if expect_operand {
            match b[i] {
                b'+' => {
                    i += 1;
                    Tok::Un(Op::Add)
                }
                b'-' => {
                    i += 1;
                    Tok::Un(Op::Sub)
                }
                b'(' => {
                    i += 1;
                    parens.push(ParenKind::Subexpr);
                    Tok::LParen
                }
                b'0'..=b'9' => {
                    let (v, ni) = parse_number(b, i)?;
                    i = ni;
                    Tok::Num(v)
                }
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                    let start = i;
                    while i < b.len() && is_ident_char(b[i]) {
                        i += 1;
                    }
                    let name = &s[start..i];
                    // 允许标识符与 '(' 之间有空白
                    let mut j = i;
                    while j < b.len() && b[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if j < b.len() && b[j] == b'(' {
                        i = j + 1;
                        parens.push(ParenKind::Func);
                        Tok::Func(name.to_string(), 0)
                    } else {
                        Tok::Var(name.to_string())
                    }
                }
                c => return Err(format!("意外的字符: {c}")),
            }
        } else {
            match b[i] {
                b'+' => {
                    i += 1;
                    Tok::Bin(Op::Add)
                }
                b'-' => {
                    i += 1;
                    Tok::Bin(Op::Sub)
                }
                b'*' => {
                    i += 1;
                    Tok::Bin(Op::Mul)
                }
                b'/' => {
                    i += 1;
                    Tok::Bin(Op::Div)
                }
                b'%' => {
                    i += 1;
                    Tok::Bin(Op::Rem)
                }
                b'^' => {
                    i += 1;
                    Tok::Bin(Op::Pow)
                }
                b')' => {
                    i += 1;
                    parens.pop().ok_or("多余的右括号")?;
                    Tok::RParen
                }
                b',' if parens.last() == Some(&ParenKind::Func) => {
                    i += 1;
                    Tok::Comma
                }
                c => return Err(format!("意外的字符: {c}")),
            }
        };
        match t {
            Tok::Num(_) | Tok::Var(_) => expect_operand = false,
            Tok::Bin(_) | Tok::Comma => expect_operand = true,
            _ => {}
        }
        toks.push(t);
    }
    if expect_operand {
        return Err("表达式不完整".into());
    }
    if !parens.is_empty() {
        return Err("缺少右括号".into());
    }
    Ok(toks)
}

/// 运算符优先级: (优先级, 是否右结合)。
fn prec(t: &Tok) -> (u32, bool) {
    match t {
        Tok::Bin(Op::Add | Op::Sub) => (1, false),
        Tok::Bin(Op::Mul | Op::Div | Op::Rem) => (2, false),
        Tok::Bin(Op::Pow) => (4, true),
        Tok::Un(_) => (3, false),
        _ => (0, false),
    }
}

/// 调度场算法: 中缀 token 序列 → RPN。
fn to_rpn(toks: &[Tok]) -> Result<Vec<Tok>, String> {
    let mut out: Vec<Tok> = Vec::with_capacity(toks.len());
    let mut stk: Vec<Tok> = Vec::with_capacity(toks.len());
    for t in toks {
        match t {
            Tok::Num(_) | Tok::Var(_) => out.push(t.clone()),
            Tok::Un(_) => stk.push(t.clone()),
            Tok::Bin(_) => {
                let (pi, right) = prec(t);
                while let Some(top) = stk.last() {
                    let (pj, _) = prec(top);
                    let pop = if right { pi < pj } else { pi <= pj };
                    if pop {
                        out.push(stk.pop().expect("栈非空"));
                    } else {
                        break;
                    }
                }
                stk.push(t.clone());
            }
            Tok::LParen => stk.push(t.clone()),
            Tok::RParen => {
                let mut found = false;
                while let Some(top) = stk.pop() {
                    match top {
                        Tok::LParen => {
                            found = true;
                            break;
                        }
                        Tok::Func(name, n) => {
                            found = true;
                            out.push(Tok::Func(name, n + 1));
                            break;
                        }
                        other => out.push(other),
                    }
                }
                if !found {
                    return Err("括号不匹配".into());
                }
            }
            Tok::Comma => {
                let mut found = false;
                while let Some(top) = stk.pop() {
                    match top {
                        Tok::LParen => return Err("逗号位置错误".into()),
                        Tok::Func(name, n) => {
                            found = true;
                            stk.push(Tok::Func(name, n + 1));
                            break;
                        }
                        other => out.push(other),
                    }
                }
                if !found {
                    return Err("逗号位置错误".into());
                }
            }
            Tok::Func(..) => stk.push(t.clone()),
        }
    }
    while let Some(top) = stk.pop() {
        match top {
            Tok::Un(_) | Tok::Bin(_) => out.push(top),
            Tok::LParen | Tok::Func(..) => return Err("缺少右括号".into()),
            _ => return Err("表达式错误".into()),
        }
    }
    Ok(out)
}

/// 栈式 RPN 求值。
fn eval_rpn(rpn: &[Tok], ctx: &ExprContext) -> Result<Cmplx, String> {
    let mut stack: Vec<Cmplx> = Vec::with_capacity(rpn.len());
    for t in rpn {
        match t {
            Tok::Num(v) => stack.push(Cmplx::real(*v)),
            Tok::Var(name) => {
                let v = ctx
                    .vars
                    .get(name)
                    .copied()
                    .ok_or_else(|| format!("未知变量: {name}"))?;
                stack.push(v);
            }
            Tok::Un(op) => {
                let x = stack.pop().ok_or("缺少操作数")?;
                stack.push(match op {
                    Op::Sub => -x,
                    _ => x,
                });
            }
            Tok::Bin(op) => {
                let b = stack.pop().ok_or("缺少操作数")?;
                let a = stack.pop().ok_or("缺少操作数")?;
                stack.push(match op {
                    Op::Add => a + b,
                    Op::Sub => a - b,
                    Op::Mul => a * b,
                    Op::Div => a / b,
                    Op::Rem => {
                        if !a.is_real() || !b.is_real() {
                            return Err("取余仅支持实数".into());
                        }
                        Cmplx::real(a.re % b.re)
                    }
                    Op::Pow => a.pow(b),
                });
            }
            Tok::Func(name, n) => {
                let n = *n;
                if stack.len() < n {
                    return Err(format!("{name} 缺少参数"));
                }
                let args = stack.split_off(stack.len() - n);
                let f = ctx
                    .funcs
                    .get(name)
                    .ok_or_else(|| format!("未知函数: {name}"))?;
                stack.push(f(&args)?);
            }
            _ => return Err("表达式错误".into()),
        }
    }
    if stack.len() != 1 {
        return Err("表达式错误".into());
    }
    Ok(stack[0])
}

/// 求值完整表达式。
pub fn eval_with_context(s: &str, ctx: &ExprContext) -> Result<Cmplx, String> {
    let toks = tokenize(s)?;
    let rpn = to_rpn(&toks)?;
    eval_rpn(&rpn, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &str) -> f64 {
        let z = eval_with_context(s, &ExprContext::new()).expect("应求值成功");
        assert!(z.is_real(), "{s} 应为实数, 实际: {z:?}");
        z.re
    }
    fn evc(s: &str) -> Cmplx {
        eval_with_context(s, &ExprContext::new()).expect("应求值成功")
    }
    fn err(s: &str) {
        assert!(
            eval_with_context(s, &ExprContext::new()).is_err(),
            "应报错: {s}"
        );
    }

    #[test]
    fn complex_ops() {
        // 注: expr 层无隐式乘法(由 math 的预处理负责), 复数一律显式写 *
        assert_eq!(evc("i^2"), Cmplx::real(-1.0));
        assert_eq!(evc("2+3*i"), Cmplx::new(2.0, 3.0));
        assert_eq!(evc("(1+2*i)*(3-4*i)"), Cmplx::new(11.0, 2.0));
        assert_eq!(evc("(3+4*i)/(1+2*i)"), Cmplx::new(2.2, -0.4));
        assert_eq!(evc("sqrt(-1)"), Cmplx::I);
        assert_eq!(evc("sqrt(-4)"), Cmplx::new(0.0, 2.0));
        assert_eq!(evc("abs(3+4*i)"), Cmplx::real(5.0));
        assert_eq!(evc("conj(1+2*i)"), Cmplx::new(1.0, -2.0));
        assert_eq!(evc("ln(-1)"), Cmplx::new(0.0, std::f64::consts::PI));
        assert_eq!(evc("e^(i*pi)"), Cmplx::real(-1.0), "e^(iπ) 应精确为 -1");
        assert_eq!(evc("1/(2*i)"), Cmplx::new(0.0, -0.5));
    }

    #[test]
    fn complex_funcs() {
        let approx = |a: Cmplx, b: Cmplx| (a - b).abs() < 1e-9;
        assert!(approx(evc("sin(i)"), Cmplx::new(0.0, 1.0f64.sinh())));
        assert!(approx(evc("cos(i)"), Cmplx::real(1.0f64.cosh())));
        assert!(approx(
            evc("asin(2)"),
            Cmplx::new(std::f64::consts::FRAC_PI_2, -1.3169578969)
        ));
        assert!(approx(
            evc("atanh(2)"),
            Cmplx::new(0.5493061443, -std::f64::consts::FRAC_PI_2)
        ));
        assert!(approx(
            evc("(1+i)^(1+i)"),
            evc("1+i").pow(Cmplx::new(1.0, 1.0))
        ));
        // 复数取余/实数专属函数报错
        err("(1+i)%2");
        err("floor(1.5+i)");
        err("max(1,2+i)");
    }

    #[test]
    fn basic_ops() {
        assert_eq!(ev("2+3*4"), 14.0);
        assert_eq!(ev("(2+3)*4"), 20.0);
        assert_eq!(ev("2^10"), 1024.0);
        assert_eq!(ev("2^3^2"), 512.0, "幂应为右结合");
        assert_eq!(ev("-2^2"), -4.0, "幂优先级应高于一元负号");
        assert_eq!(ev("2^-2"), 0.25);
        assert_eq!(ev("5+-3"), 2.0);
        assert_eq!(ev("2*-3"), -6.0);
        assert_eq!(ev("1-2*3"), -5.0);
        assert_eq!(ev("2/3*4"), 8.0 / 3.0);
        assert_eq!(ev("10%3"), 1.0);
        assert_eq!(ev("+5"), 5.0);
        assert_eq!(ev("1/0"), f64::INFINITY);
    }

    #[test]
    fn numbers() {
        assert_eq!(ev("2e3"), 2000.0);
        assert_eq!(ev("2.5e-2"), 0.025);
        assert_eq!(ev("1e999"), f64::INFINITY);
        assert_eq!(ev("2.5E2"), 250.0);
        assert_eq!(ev("2."), 2.0);
    }

    #[test]
    fn funcs_and_vars() {
        assert_eq!(ev("sin(pi/6)"), 0.49999999999999994);
        assert_eq!(ev("ln(e)"), 1.0);
        assert_eq!(ev("atan2(1,1)"), std::f64::consts::FRAC_PI_4);
        assert_eq!(ev("max(1,2,3)"), 3.0);
        assert_eq!(ev("min(1,2)"), 1.0);
        assert_eq!(ev("sqrt(16)"), 4.0);
        assert_eq!(ev("max(5)"), 5.0);
    }

    #[test]
    fn malformed() {
        err("");
        err("2+");
        err("2**3");
        err("()");
        err("2)");
        err("(((2)");
        err("f(2,)");
        err("f(,2)");
        err("1,2");
        err("foo(2)");
        err("x");
        err("sin(1,2)");
        err("max()");
        err("sin()");
        err("2 3");
    }
}

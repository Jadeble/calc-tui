//! 表达式预处理与求值。
//!
//! 核心思路:
//! - 使用 meval 作为表达式解析/求值引擎(支持 + - * / ^、一元正负号、函数调用、变量)。
//! - 在交给 meval 之前做字符串级预处理,依次完成:
//!   1. 符号映射: × → *, ÷ → /, π → (pi), √( → sqrt(, √数字 → sqrt(数字),
//!      ° → *pi/180, Σ → sum(, ∫ → int(
//!   2. 复合结构替换: sum(变量,起始,结束,表达式) / int(变量,下,上,表达式) /
//!      deriv(表达式,变量,取值点) 被数值化替换(可嵌套,内部递归预处理)。
//!   3. 阶乘: n! / (expr)! → fact(n) / fact((expr))。
//!   4. 隐式乘法: 2pi → 2*pi, 2(3+4) → 2*(3+4), 2sin(x) → 2*sin(x) 等
//!      (跳过科学计数法 2e3 与函数名 sin( 等情况)。
//!   5. 角度模式: sin( → sin_d( 等(DEG 模式下三角函数按角度计算,
//!      反三角函数结果以度为单位)。

use meval::Context;
use std::f64::consts::PI;

/// 求值器。degree 为 true 时三角函数按角度(度)计算。
pub struct Evaluator {
    pub degree: bool,
}

impl Evaluator {
    pub fn new(degree: bool) -> Self {
        Self { degree }
    }

    pub fn evaluate(&self, expr: &str) -> Result<f64, String> {
        let s = preprocess(expr, self.degree)?;
        let v = eval_str(&s)?;
        if v.is_nan() {
            Err("结果未定义 (NaN): 请检查函数定义域或阶乘参数".into())
        } else {
            Ok(v)
        }
    }
}

// ---------------------------------------------------------------- 预处理

fn preprocess(s: &str, degree: bool) -> Result<String, String> {
    let mut s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if s.is_empty() {
        return Ok(s);
    }
    map_symbols(&mut s, degree);
    substitute_constructs(&mut s, degree)?;
    factorial_pass(&mut s, degree);
    implicit_mult(&mut s);
    if degree {
        degree_pass(&mut s);
    }
    Ok(s)
}

/// 显示字符 → 可解析字符 的映射。
fn map_symbols(s: &mut String, degree: bool) {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 8);
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '×' => out.push('*'),
            '÷' => out.push('/'),
            'π' => out.push_str("(pi)"),
            // 30° = 30 度: DEG 模式下数值已是度, RAD 模式下换算为弧度
            '°' if degree => {}
            '°' => out.push_str("*pi/180"),
            'Σ' => out.push_str("sum"),
            '∫' => out.push_str("int"),
            '√' => {
                if i + 1 < chars.len() && chars[i + 1] == '(' {
                    out.push_str("sqrt");
                } else {
                    out.push_str("sqrt(");
                    let mut j = i + 1;
                    while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '.') {
                        out.push(chars[j]);
                        j += 1;
                    }
                    out.push(')');
                    i = j - 1;
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    *s = out;
}

const CONSTRUCTS: &[&str] = &["sum", "int", "deriv"];

/// 找到最左侧的复合结构关键字(要求后随 '(' 且前有标识符边界)。
fn find_construct(s: &str) -> Option<(usize, &'static str)> {
    let bytes = s.as_bytes();
    for idx in 0..bytes.len() {
        for kw in CONSTRUCTS {
            let klen = kw.len();
            if idx + klen + 1 > bytes.len() {
                continue;
            }
            if &bytes[idx..idx + klen] == kw.as_bytes()
                && bytes[idx + klen] == b'('
                && (idx == 0 || !is_ident_char(bytes[idx - 1] as char))
            {
                return Some((idx, kw));
            }
        }
    }
    None
}

/// 返回与 open(open 指向 '(') 匹配的 ')' 的字节下标。
fn matching_paren(s: &str, open: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0usize;
    for (i, &b) in bytes.iter().enumerate().skip(open) {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// 按顶层逗号切分参数(括号内不切分)。
fn split_top_level(s: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return Err("括号不匹配".into());
                }
                cur.push(c);
            }
            ',' if depth == 0 => {
                args.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if depth != 0 {
        return Err("括号不匹配".into());
    }
    args.push(cur.trim().to_string());
    Ok(args)
}

/// 把 sum/int/deriv 结构替换为数值。
fn substitute_constructs(s: &mut String, degree: bool) -> Result<(), String> {
    loop {
        let Some((start, name)) = find_construct(s) else {
            return Ok(());
        };
        let open = start + name.len();
        let Some(close) = matching_paren(s, open) else {
            return Err(format!("{name}( 缺少右括号"));
        };
        let args = split_top_level(&s[open + 1..close])?;
        let val = match name {
            "sum" => eval_sum(&args, degree)?,
            "int" => eval_int(&args, degree)?,
            "deriv" => eval_deriv(&args, degree)?,
            _ => unreachable!(),
        };
        if !val.is_finite() {
            return Err(format!("{name} 的计算结果不是有限数值"));
        }
        *s = format!("{}{}{}", &s[..start], fmt_num(val), &s[close + 1..]);
    }
}

/// 把 n! 与 (expr)! 替换为 fact(...)。支持连续阶乘: 3!! → fact(fact(3))。
fn factorial_pass(s: &mut String, degree: bool) {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 16);
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '!' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        // 从 '!' 开始向前回溯操作数
        let mut j = i;
        let operand_start = loop {
            if j == 0 {
                break None;
            }
            match chars[j - 1] {
                '!' => j -= 1, // 内层阶乘并入操作数
                ')' => {
                    let mut depth = 1i32;
                    j -= 1;
                    while j > 0 {
                        j -= 1;
                        match chars[j] {
                            ')' => depth += 1,
                            '(' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    break (depth == 0).then_some(j);
                }
                c if c.is_ascii_digit() || c == '.' => {
                    j -= 1;
                    while j > 0
                        && (chars[j - 1].is_ascii_digit()
                            || chars[j - 1] == '.'
                            || chars[j - 1] == '!')
                    {
                        j -= 1;
                    }
                    break Some(j);
                }
                _ => break None,
            }
        };
        let Some(start) = operand_start else {
            out.push('!');
            i += 1;
            continue;
        };
        // 截掉已输出的操作数部分(均为原样输出),再包一层 fact(...)
        let cut = chars[..start].iter().map(|c| c.len_utf8()).sum::<usize>();
        out.truncate(cut);
        let operand: String = chars[start..i].iter().collect();
        match preprocess(&operand, degree) {
            Ok(inner) => out.push_str(&format!("fact({inner})")),
            Err(_) => {
                out.push_str(&operand);
                out.push('!');
            }
        }
        i += 1;
    }
    *s = out;
}

/// 隐式乘法: 在相邻的"数值结尾"与"表达式开头"之间插入 '*',跳过科学计数法与函数名。
fn implicit_mult(s: &mut String) {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(s.len() + 8);
    for i in 0..n {
        if chars[i] == '.' && (i == 0 || !(chars[i - 1].is_ascii_digit() || chars[i - 1] == '.')) {
            out.push('0');
        }
        if i > 0 && needs_star(&chars, i) {
            out.push('*');
        }
        out.push(chars[i]);
    }
    *s = out;
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}
fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// 已注册到 meval 上下文中的函数名(用于判断标识符后紧跟 '(' 时是否需要插入 '*')。
const FUNC_NAMES: &[&str] = &[
    "abs", "exp", "ln", "sqrt", "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh",
    "tanh", "asinh", "acosh", "atanh", "floor", "ceil", "round", "signum", "atan2", "max", "min",
    "fact", "log", "logb", "log2", "log10", "sin_d", "cos_d", "tan_d", "asin_d", "acos_d",
    "atan_d", "atan2_d",
];

/// 判断 chars[i-1] 与 chars[i] 之间是否需要插入 '*'(i > 0)。
fn needs_star(chars: &[char], i: usize) -> bool {
    let n = chars.len();
    let l = chars[i - 1];
    let r = chars[i];
    let is_dig = |c: char| c.is_ascii_digit();

    // 科学计数法: 2e3, 2e+3, 2.5e-2, 1e5 等
    if l == 'e'
        && i >= 2
        && (is_dig(chars[i - 2]) || chars[i - 2] == '.')
        && (is_dig(r) || ((r == '+' || r == '-') && i + 1 < n && is_dig(chars[i + 1])))
    {
        return false;
    }
    if r == 'e' && is_dig(l) && i + 1 < n && is_dig(chars[i + 1]) {
        return false;
    }
    if r == 'e'
        && is_dig(l)
        && i + 1 < n
        && (chars[i + 1] == '+' || chars[i + 1] == '-')
        && i + 2 < n
        && is_dig(chars[i + 2])
    {
        return false;
    }

    // ')' 结尾 → 表达式开头
    if l == ')' && (is_dig(r) || r == '(' || is_ident_start(r)) {
        return true;
    }
    // 数字 + 字母/下划线: 2pi → 2*pi
    if is_dig(l) && is_ident_start(r) {
        return true;
    }
    // 字母 + 数字: x2 → x*2 (log2、atan2、log10 等函数名除外)
    if is_ident_start(l) && is_dig(r) {
        let mut rs = i;
        while rs > 0 && is_ident_char(chars[rs - 1]) {
            rs -= 1;
        }
        let mut re = i + 1;
        while re < n && is_dig(chars[re]) {
            re += 1;
        }
        let run: String = chars[rs..re].iter().collect();
        if !FUNC_NAMES.contains(&run.as_str()) {
            return true;
        }
    }
    // 以 '(' 结尾的标识符段: sin( atan2( 等函数名不插入; 其余插入
    if is_ident_char(l) && r == '(' {
        let mut rs = i;
        while rs > 0 && is_ident_char(chars[rs - 1]) {
            rs -= 1;
        }
        let run: String = chars[rs..i].iter().collect();
        if FUNC_NAMES.contains(&run.as_str()) {
            return false;
        }
        let starts_digit = run.chars().next().is_some_and(is_dig);
        if starts_digit && run.chars().any(|c| !is_dig(c) && c != '_') {
            return false;
        }
        return true;
    }
    false
}

/// DEG 模式下把三角/反三角函数替换为度版本。
fn degree_pass(s: &mut String) {
    const MAP: &[(&str, &str)] = &[
        ("asin", "asin_d"),
        ("acos", "acos_d"),
        ("atan", "atan_d"),
        ("atan2", "atan2_d"),
        ("sin", "sin_d"),
        ("cos", "cos_d"),
        ("tan", "tan_d"),
    ];
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if is_ident_start(c) {
            let mut j = i;
            while j < chars.len() && is_ident_char(chars[j]) {
                j += 1;
            }
            let name: String = chars[i..j].iter().collect();
            let repl = if j < chars.len() && chars[j] == '(' {
                MAP.iter().find(|(a, _)| *a == name).map(|(_, b)| *b)
            } else {
                None
            };
            match repl {
                Some(r) => out.push_str(r),
                None => out.push_str(&name),
            }
            i = j;
        } else {
            out.push(c);
            i += 1;
        }
    }
    *s = out;
}

// ---------------------------------------------------------------- 求值

fn base_context() -> Context<'static> {
    let mut ctx = Context::new();
    ctx.func("fact", fact);
    ctx.func("log", |x: f64| x.log10());
    ctx.func("log2", |x: f64| x.log2());
    ctx.func("log10", |x: f64| x.log10());
    ctx.func2("logb", |b: f64, x: f64| {
        if b > 0.0 && b != 1.0 {
            x.ln() / b.ln()
        } else {
            f64::NAN
        }
    });
    ctx.func("sin_d", |x: f64| (x * PI / 180.0).sin());
    ctx.func("cos_d", |x: f64| (x * PI / 180.0).cos());
    ctx.func("tan_d", |x: f64| (x * PI / 180.0).tan());
    ctx.func("asin_d", |x: f64| x.asin() * 180.0 / PI);
    ctx.func("acos_d", |x: f64| x.acos() * 180.0 / PI);
    ctx.func("atan_d", |x: f64| x.atan() * 180.0 / PI);
    ctx.func2("atan2_d", |y: f64, x: f64| y.atan2(x) * 180.0 / PI);
    ctx
}

fn eval_str(s: &str) -> Result<f64, String> {
    let ctx = base_context();
    meval::eval_str_with_context(s, &ctx).map_err(|e| format!("表达式错误: {e}"))
}

/// 对一个"参数片段"做完整预处理并求值(片段内允许嵌套复合结构)。
fn eval_plain(s: &str, degree: bool) -> Result<f64, String> {
    let p = preprocess(s, degree)?;
    eval_str(&p)
}

fn validate_var(var: &str) -> Result<(), String> {
    let mut cs = var.chars();
    match cs.next() {
        Some(c) if is_ident_start(c) => {}
        _ => return Err(format!("'{var}' 不是合法的变量名(应为字母开头)")),
    }
    for c in cs {
        if !is_ident_char(c) {
            return Err(format!("'{var}' 不是合法的变量名"));
        }
    }
    Ok(())
}

/// 把表达式中独立的 var 出现替换为数值(边界匹配,不替换 var2、xvar 等)。
fn subst_var(s: &str, var: &str, val: f64) -> String {
    if var.is_empty() {
        return s.to_string();
    }
    let v = fmt_num(val);
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 8);
    let mut i = 0;
    while i < chars.len() {
        if i + var.chars().count() <= chars.len() {
            let seg: String = chars[i..i + var.chars().count()].iter().collect();
            if seg == var {
                // 左侧: 变量名前不能是字母/下划线(数字可以, 2x = 2*x)
                let prev_ok =
                    i == 0 || !(chars[i - 1].is_ascii_alphabetic() || chars[i - 1] == '_');
                let nxt = i + var.chars().count();
                // 右侧: 不能是标识符字符 (x2 是变量 x2, 不替换)
                let next_ok = nxt >= chars.len() || !is_ident_char(chars[nxt]);
                if prev_ok && next_ok {
                    // 2x → 2*{val} (直接替换会拼成 21)
                    let need_star = i > 0
                        && (chars[i - 1].is_ascii_digit()
                            || chars[i - 1] == '.'
                            || chars[i - 1] == ')');
                    if need_star {
                        out.push('*');
                    }
                    out.push_str(&v);
                    i = nxt;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn eval_sum(args: &[String], degree: bool) -> Result<f64, String> {
    if args.len() != 4 {
        return Err("Σ 需要4个参数: Σ(变量, 起始, 结束, 表达式)".into());
    }
    let var = args[0].trim();
    validate_var(var)?;
    let start = eval_plain(&args[1], degree)?;
    let end = eval_plain(&args[2], degree)?;
    let mut k = start.ceil();
    let stop = end.floor();
    if k > stop {
        return Ok(0.0);
    }
    if stop - k > 1_000_000.0 {
        return Err("求和项数过多 (上限 1,000,000)".into());
    }
    let mut total = 0.0;
    while k <= stop {
        let term = subst_var(&args[3], var, k);
        let pp = preprocess(&term, degree)?;
        let t = eval_str(&pp).map_err(|e| format!("求和项错误 (k={k}): {e}"))?;
        total += t;
        k += 1.0;
    }
    Ok(total)
}

fn simpson_step(
    f: &mut impl FnMut(f64) -> Result<f64, String>,
    a: f64,
    b: f64,
    n: usize,
) -> Result<f64, String> {
    let h = (b - a) / n as f64;
    let mut s = f(a)? + f(b)?;
    for i in 1..n {
        let x = a + i as f64 * h;
        let w = if i % 2 == 1 { 4.0 } else { 2.0 };
        s += w * f(x)?;
    }
    Ok(s * h / 3.0)
}

fn adaptive_simpson(
    mut f: impl FnMut(f64) -> Result<f64, String>,
    a: f64,
    b: f64,
) -> Result<f64, String> {
    if a == b {
        return Ok(0.0);
    }
    let mut n = 64usize;
    let mut prev = simpson_step(&mut f, a, b, n)?;
    loop {
        n *= 2;
        if n > (1 << 22) {
            return Err("积分未收敛 (子区间过多)".into());
        }
        let cur = simpson_step(&mut f, a, b, n)?;
        if (cur - prev).abs() <= 1e-9 * (1.0 + cur.abs()) {
            return Ok(cur);
        }
        prev = cur;
    }
}

fn eval_int(args: &[String], degree: bool) -> Result<f64, String> {
    if args.len() != 4 {
        return Err("∫ 需要4个参数: ∫(变量, 下限, 上限, 表达式)".into());
    }
    let var = args[0].trim();
    validate_var(var)?;
    let a = eval_plain(&args[1], degree)?;
    let b = eval_plain(&args[2], degree)?;
    let expr = args[3].clone();
    adaptive_simpson(
        move |x: f64| {
            let s = subst_var(&expr, var, x);
            let pp = preprocess(&s, degree)?;
            eval_str(&pp)
        },
        a,
        b,
    )
}

fn eval_deriv(args: &[String], degree: bool) -> Result<f64, String> {
    if args.len() != 3 {
        return Err("deriv 需要3个参数: deriv(表达式, 变量, 取值点x0)".into());
    }
    let var = args[1].trim();
    validate_var(var)?;
    let x0 = eval_plain(&args[2], degree)?;
    let expr = args[0].clone();
    let h = 1e-4;
    let f = |x: f64| -> Result<f64, String> {
        let s = subst_var(&expr, var, x);
        let pp = preprocess(&s, degree)?;
        eval_str(&pp)
    };
    let m2 = f(x0 - 2.0 * h)?;
    let m1 = f(x0 - h)?;
    let p1 = f(x0 + h)?;
    let p2 = f(x0 + 2.0 * h)?;
    Ok((m2 - 8.0 * m1 + 8.0 * p1 - p2) / (12.0 * h))
}

fn fact(x: f64) -> f64 {
    // 非负整数以外 → 未定义; 超出 f64 上限 (171! ≈ 1.24e309) → 无穷大
    if x < 0.0 || x.fract() != 0.0 {
        return f64::NAN;
    }
    if x > 170.0 {
        return f64::INFINITY;
    }
    let n = x as u64;
    let mut r = 1.0;
    for i in 2..=n {
        r *= i as f64;
    }
    r
}

// ---------------------------------------------------------------- 格式化

/// 表达式内替换使用的紧凑数值(保证 meval 可解析)。
pub fn fmt_num(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 {
            "1e999".into()
        } else {
            "-1e999".into()
        };
    }
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{:.0}", v);
    }
    if v != 0.0 && (v.abs() >= 1e12 || v.abs() < 1e-6) {
        let s = format!("{:.10e}", v);
        let (m, e) = s.split_once('e').expect("sci format has e");
        let m = m.trim_end_matches('0').trim_end_matches('.');
        format!("{m}e{e}")
    } else {
        let s = format!("{:.10}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// 结果展示用数值。
pub fn fmt_result(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return "∞".into();
    }
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{:.0}", v);
    }
    if v != 0.0 && (v.abs() >= 1e10 || v.abs() < 1e-5) {
        let s = format!("{:.6e}", v);
        let (m, e) = s.split_once('e').expect("sci format has e");
        let m = m.trim_end_matches('0').trim_end_matches('.');
        format!("{m}e{e}")
    } else {
        let s = format!("{:.10}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

// ---------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn ok(e: &str) -> f64 {
        Evaluator::new(false).evaluate(e).expect("应求值成功")
    }
    fn deg(e: &str) -> f64 {
        Evaluator::new(true).evaluate(e).expect("应求值成功")
    }
    fn err(e: &str, d: bool) {
        assert!(Evaluator::new(d).evaluate(e).is_err(), "应报错: {e}");
    }

    #[test]
    fn basic_ops() {
        assert_eq!(ok("2+3*4"), 14.0);
        assert_eq!(ok("(2+3)*4"), 20.0);
        assert_eq!(ok("2^10"), 1024.0);
        assert_eq!(ok("5+-3"), 2.0);
        assert_eq!(ok("-2^2"), -4.0);
    }

    #[test]
    fn implicit_mul() {
        assert!(approx(ok("2pi"), 2.0 * std::f64::consts::PI));
        assert!(approx(ok("2π"), 2.0 * std::f64::consts::PI));
        assert_eq!(ok("2(3+4)"), 14.0);
        assert_eq!(ok("(1+2)(3+4)"), 21.0);
        assert!(approx(ok("2sin(pi/2)"), 2.0));
        assert_eq!(ok("2e3"), 2000.0);
        assert_eq!(ok("2.5e-2"), 0.025);
        assert!(approx(ok("2e"), 2.0 * std::f64::consts::E));
        assert!(approx(ok("e2"), 2.0 * std::f64::consts::E));
        assert_eq!(ok("3!!"), 720.0);
        assert_eq!(ok("5!2"), 240.0);
    }

    #[test]
    fn factorial() {
        assert_eq!(ok("5!"), 120.0);
        assert_eq!(ok("(2+3)!"), 120.0);
        assert_eq!(ok("0!"), 1.0);
        assert_eq!(ok("2^3!"), 64.0);
        assert!(ok("170!").is_finite(), "170! 应可表示");
        assert_eq!(ok("171!"), f64::INFINITY);
        err("5.5!", false);
        err("(-1)!", false);
    }

    #[test]
    fn trig() {
        assert!(approx(deg("sin(30)"), 0.5));
        assert!(approx(deg("cos(60)"), 0.5));
        assert!(approx(deg("tan(45)"), 1.0));
        assert!(approx(deg("asin(0.5)"), 30.0));
        assert!(approx(deg("acos(0.5)"), 60.0));
        assert!(approx(deg("atan(1)"), 45.0));
        assert!(approx(deg("sin(30°)+sin(30)"), 1.0));
        assert!(approx(ok("sin(pi/6)"), 0.5));
        assert!(approx(ok("sin(0.5235987756)"), 0.5));
        assert!(approx(ok("atan2(1,1)"), std::f64::consts::FRAC_PI_4));
        assert!(approx(deg("atan2(1,1)"), 45.0));
        assert!(approx(ok("sinh(0)"), 0.0));
    }

    #[test]
    fn log_exp() {
        assert_eq!(ok("log(100)"), 2.0);
        assert_eq!(ok("logb(2,8)"), 3.0);
        assert_eq!(ok("logb(2,8)+log2(8)"), 6.0);
        assert_eq!(ok("log10(1000)"), 3.0);
        assert_eq!(ok("ln(e)"), 1.0);
        assert_eq!(ok("exp(0)"), 1.0);
        assert_eq!(ok("sqrt(16)"), 4.0);
        assert_eq!(ok("√(16)"), 4.0);
        assert_eq!(ok("√16"), 4.0);
        err("logb(1,8)", false);
    }

    #[test]
    fn sum() {
        assert_eq!(ok("Σ(k,1,10,k^2)"), 385.0);
        assert_eq!(ok("sum(k,1,10,k)"), 55.0);
        assert_eq!(ok("Σ(k,5,2,k)"), 0.0);
        assert_eq!(ok("1+Σ(k,1,3,k)"), 7.0);
        assert_eq!(ok("Σ(k,1,3,Σ(j,1,k,j))"), 10.0);
        assert_eq!(ok("Σ(k,1,4,k!)"), 33.0);
        assert!(approx(ok("Σ(k,1,3,sin_d(k))"), deg("sin(1)+sin(2)+sin(3)")));
        err("Σ(k,1,10)", false);
        err("Σ(1,1,10,k)", false);
    }

    #[test]
    fn integral() {
        assert!(approx(ok("∫(x,0,1,x^2)"), 1.0 / 3.0));
        assert!(approx(ok("int(x,0,1,x^2)"), 1.0 / 3.0));
        assert!(approx(ok("∫(x,0,pi,sin(x))"), 2.0));
        assert!(approx(ok("∫(x,1,2,1/x)"), 2f64.ln()));
        assert!(approx(ok("∫(x,2,2,x)"), 0.0));
        err("∫(x,0,1)", false);
    }

    #[test]
    fn derivative() {
        assert!(approx(ok("deriv(x^2,x,2)"), 4.0));
        assert!(approx(ok("deriv(sin(x),x,0)"), 1.0));
        assert!(approx(ok("deriv(exp(x),x,0)"), 1.0));
        assert!(approx(ok("deriv(∫(y,0,x,1),x,2)"), 1.0));
        err("deriv(x^2,x)", false);
    }

    #[test]
    fn nested_constructs() {
        // Σ 的每一项内再做定积分: Σ k=1..3 ∫(x,0,k,x^2) = Σ k^3/3 = (1+8+27)/3 = 12
        assert!(approx(ok("Σ(k,1,3,∫(x,0,k,x^2))"), 12.0));
        // 对含 Σ 的表达式求导
        assert!(approx(ok("deriv(Σ(k,1,4,k*x),x,1)"), 10.0));
        // 积分内使用隐式乘法
        assert!(approx(ok("∫(x,0,2,2x)"), 4.0));
    }

    #[test]
    fn misc() {
        assert_eq!(ok(".5"), 0.5);
        assert!(approx(ok("sin(.5)"), 0.5f64.sin()));
        assert!(approx(ok("2pi*2"), 4.0 * std::f64::consts::PI));
    }

    #[test]
    fn errors() {
        err("sin(", false);
        err("Σ(k,1,10,k^2", false);
        err("2+", false);
        err("2**3", false);
    }

    #[test]
    fn fmt() {
        assert_eq!(fmt_num(6.0), "6");
        assert_eq!(fmt_num(-0.5), "-0.5");
        assert_eq!(fmt_num(0.3333333333333333), "0.3333333333");
    }
}

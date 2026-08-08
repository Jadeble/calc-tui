//! 表达式预处理与求值。
//!
//! 核心思路:
//! - 使用仓库内自研的 expr 模块(分词 + 调度场算法)作为表达式解析/求值引擎。
//! - 在交给求值器之前做字符串级预处理,依次完成:
//!   1. 符号映射: × → *, ÷ → /, π → (pi), √( → sqrt(, √数字 → sqrt(数字),
//!      ° → *pi/180, Σ → sum(, ∫ → int(
//!   2. 复合结构替换: sum(变量,起始,结束,表达式) / int(变量,下,上,表达式) /
//!      deriv(表达式,变量,取值点) 被数值化替换(可嵌套,内部递归预处理)。
//!   3. 阶乘: n! / (expr)! → fact(n) / fact((expr))。
//!   4. 隐式乘法: 2pi → 2*pi, 2(3+4) → 2*(3+4), 2sin(x) → 2*sin(x) 等
//!      (跳过科学计数法 2e3 与函数名 sin( 等情况)。
//!   5. 角度模式: sin( → sin_d( 等(DEG 模式下三角函数按角度计算,
//!      反三角函数结果以度为单位)。

use crate::complex::{Cmplx, ccos, csin, ctan, sin_snap, tan_snap_at};
use crate::config::UserFunc;
use crate::expr::{ExprContext, eval_with_context};
use std::f64::consts::PI;

/// 求值器。degree 为 true 时三角函数按角度(度)计算; funcs 为自定义函数列表。
pub struct Evaluator {
    pub degree: bool,
    pub funcs: Vec<UserFunc>,
}

impl Evaluator {
    pub fn new(degree: bool) -> Self {
        Self {
            degree,
            funcs: Vec::new(),
        }
    }

    pub fn evaluate(&self, expr: &str) -> Result<Cmplx, String> {
        let s = preprocess(expr, self.degree, &self.funcs, 0)?;
        let v = eval_str(&s)?;
        if v.is_nan() {
            Err("结果未定义 (NaN): 请检查函数定义域或阶乘参数".into())
        } else {
            Ok(v)
        }
    }
}

// ---------------------------------------------------------------- 预处理

/// 预处理嵌套深度上限(自定义函数展开/递归时防止死循环)。
const MAX_DEPTH: usize = 64;

fn preprocess(s: &str, degree: bool, funcs: &[UserFunc], depth: usize) -> Result<String, String> {
    if depth > MAX_DEPTH {
        return Err("嵌套过深 (超过 64 层)".into());
    }
    let mut s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if s.is_empty() {
        return Ok(s);
    }
    map_symbols(&mut s, degree);
    abs_pass(&mut s);
    substitute_constructs(&mut s, degree, funcs, depth)?;
    factorial_pass(&mut s, degree, funcs, depth);
    expand_user_funcs(&mut s, degree, funcs, depth)?;
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
            'φ' => out.push_str("phi"),
            // 30° = 30 度: DEG 模式下数值已是度, RAD 模式下换算为弧度
            '°' if degree => {}
            '°' => out.push_str("*pi/180"),
            'Σ' => out.push_str("sum"),
            '∫' => out.push_str("int"),
            '∞' => out.push_str("1e999"),
            'γ' => out.push_str("euler"),
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

/// 把 |expr| 替换为 abs(expr)(相邻配对, 支持 |a|+|b| 等形式)。
fn abs_pass(s: &mut String) {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 8);
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '|' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '|' {
                j += 1;
            }
            if j < chars.len() {
                let inner: String = chars[i + 1..j].iter().collect();
                out.push_str(&format!("abs({inner})"));
                i = j + 1;
                continue;
            }
        }
        out.push(chars[i]);
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
fn substitute_constructs(
    s: &mut String,
    degree: bool,
    funcs: &[UserFunc],
    depth: usize,
) -> Result<(), String> {
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
            "sum" => eval_sum(&args, degree, funcs, depth)?,
            "int" => eval_int(&args, degree, funcs, depth)?,
            "deriv" => eval_deriv(&args, degree, funcs, depth)?,
            _ => unreachable!(),
        };
        if !val.is_finite() {
            return Err(format!("{name} 的计算结果不是有限数值"));
        }
        *s = format!("{}{}{}", &s[..start], fmt_num(val), &s[close + 1..]);
    }
}

/// 把 n! 与 (expr)! 替换为 fact(...)。支持连续阶乘: 3!! → fact(fact(3))。
fn factorial_pass(s: &mut String, degree: bool, funcs: &[UserFunc], depth: usize) {
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
        match preprocess(&operand, degree, funcs, depth + 1) {
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

/// 已注册到求值上下文中的函数/常量名(用于判断标识符后紧跟 '(' 或数字时
/// 是否需要插入 '*'; ln2、sqrt2 等带数字的常量名也必须在此列)。
const FUNC_NAMES: &[&str] = &[
    "abs", "exp", "ln", "sqrt", "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh",
    "tanh", "asinh", "acosh", "atanh", "floor", "ceil", "round", "signum", "atan2", "max", "min",
    "fact", "mod", "log", "logb", "log2", "log10", "sin_d", "cos_d", "tan_d", "asin_d", "acos_d",
    "atan_d", "atan2_d", "conj", "arg", "re", "im", "sec", "csc", "cot", "sech", "csch", "coth",
    "C", "A", "gamma", "frac", "ln2", "ln10", "sqrt2", "euler", "mean", "var", "std",
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

/// 找到最左侧的自定义函数调用 f_n(, 返回 (名字起始字节下标, 名字)。
/// 名字按长度降序匹配(f_10 优先于 f_1); 左侧只拦字母/下划线
/// (数字前的调用属于隐式乘法, 如 2f_1(3)), 右侧要求非标识符字符。
fn find_user_func_call<'a>(s: &str, funcs: &'a [UserFunc]) -> Option<(usize, &'a str)> {
    let b = s.as_bytes();
    let mut names: Vec<&str> = funcs.iter().map(|f| f.name.as_str()).collect();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    let mut i = 0;
    while i < b.len() {
        if i > 0 && (b[i - 1].is_ascii_alphabetic() || b[i - 1] == b'_') {
            i += 1;
            continue;
        }
        for name in &names {
            let nb = name.as_bytes();
            if b.len() - i >= nb.len() && &b[i..i + nb.len()] == nb {
                let after = i + nb.len();
                if after < b.len() && b[after] == b'(' {
                    return Some((i, name));
                }
            }
        }
        i += 1;
    }
    None
}

/// 展开自定义函数调用 f_n(参数): 参数先求值为数值, 代入函数体(以 x 为参数),
/// 再对替换结果递归走完整预处理(函数体内的 Σ/阶乘/隐式乘法/角度模式均正确处理)。
/// 放在 implicit_mult 之前(f_1( 不会被误插 *, 展开结果由后续隐式乘法补星),
/// 放在 degree_pass 之前(函数体内三角函数的模式跟随由递归预处理保证)。
/// 函数体以括号包裹, 保证 f_1(2)*3 不会因优先级变成 f_1(2)+1*3。
fn expand_user_funcs(
    s: &mut String,
    degree: bool,
    funcs: &[UserFunc],
    depth: usize,
) -> Result<(), String> {
    if funcs.is_empty() {
        return Ok(());
    }
    loop {
        let Some((start, name)) = find_user_func_call(s, funcs) else {
            return Ok(());
        };
        let open = start + name.len();
        let Some(close) = matching_paren(s, open) else {
            return Err(format!("{name}( 缺少右括号"));
        };
        let arg_text = &s[open + 1..close];
        let args = split_top_level(arg_text)?;
        if args.len() != 1 || args[0].trim().is_empty() {
            return Err(format!("{name} 需要 1 个参数"));
        }
        let arg_val = eval_plain(args[0].trim(), degree, funcs, depth + 1)?;
        if arg_val.is_nan() {
            return Err(format!("{name} 的参数未定义"));
        }
        let body = funcs
            .iter()
            .find(|f| f.name == name)
            .expect("名字来自 funcs")
            .body
            .clone();
        let substituted = subst_var(&body, "x", arg_val);
        let processed = preprocess(&format!("({substituted})"), degree, funcs, depth + 1)?;
        s.replace_range(start..close + 1, &processed);
    }
}

/// 判断文本中是否以独立标识符形式引用函数 name(f_2 算, f_20/f_2x 不算)。
pub fn func_name_referenced(text: &str, name: &str) -> bool {
    let b = text.as_bytes();
    let nb = name.as_bytes();
    let mut i = 0;
    while i + nb.len() <= b.len() {
        if &b[i..i + nb.len()] == nb {
            let prev_ok = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
            let next = i + nb.len();
            let next_ok = next >= b.len() || !(b[next].is_ascii_alphanumeric() || b[next] == b'_');
            if prev_ok && next_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------- 求值

/// DEG 模式下先按周期 360° 归约, 大角度也能精确命中整倍 π/2。
fn sin_d(z: Cmplx) -> Cmplx {
    if z.is_real() {
        Cmplx::real(sin_snap(z.re.rem_euclid(360.0) * PI / 180.0))
    } else {
        csin(z * (PI / 180.0))
    }
}

fn cos_d(z: Cmplx) -> Cmplx {
    if z.is_real() {
        Cmplx::real(cos_snap_deg(z.re))
    } else {
        ccos(z * (PI / 180.0))
    }
}

fn tan_d(z: Cmplx) -> Cmplx {
    if z.is_real() {
        Cmplx::real(tan_snap_at(
            z.re.rem_euclid(360.0) * PI / 180.0,
            z.re * PI / 180.0,
        ))
    } else {
        ctan(z * (PI / 180.0))
    }
}

/// DEG 模式的 cos 精确矫正(与 sin 共用周期归约)。
fn cos_snap_deg(x: f64) -> f64 {
    use crate::complex::cos_snap;
    cos_snap(x.rem_euclid(360.0) * PI / 180.0)
}

/// 反三角的 DEG 版本(复数主分支)。
fn asin_d(z: Cmplx) -> Cmplx {
    crate::complex::casin(z) * (180.0 / PI)
}

fn acos_d(z: Cmplx) -> Cmplx {
    crate::complex::cacos(z) * (180.0 / PI)
}

fn atan_d(z: Cmplx) -> Cmplx {
    crate::complex::catan(z) * (180.0 / PI)
}

fn base_context() -> ExprContext {
    let mut ctx = ExprContext::new();
    ctx.func_real("fact", fact);
    ctx.func("log", |z: Cmplx| {
        if z.is_real() {
            Cmplx::real(z.re.log10())
        } else {
            z.ln() / 10.0f64.ln()
        }
    });
    ctx.func("log2", |z: Cmplx| {
        if z.is_real() {
            Cmplx::real(z.re.log2())
        } else {
            z.ln() / 2.0f64.ln()
        }
    });
    ctx.func("log10", |z: Cmplx| {
        if z.is_real() {
            Cmplx::real(z.re.log10())
        } else {
            z.ln() / 10.0f64.ln()
        }
    });
    ctx.func2("logb", |b: Cmplx, x: Cmplx| {
        if b.is_real() && b.re > 0.0 && b.re != 1.0 {
            if x.is_real() && x.re > 0.0 {
                Cmplx::real(x.re.ln() / b.re.ln())
            } else {
                x.ln() / b.ln()
            }
        } else {
            Cmplx::nan()
        }
    });
    ctx.func2_real(
        "mod",
        |x: f64, y: f64| {
            if y == 0.0 { f64::NAN } else { x % y }
        },
    );
    ctx.func("sin", crate::complex::csin);
    ctx.func("cos", crate::complex::ccos);
    ctx.func("tan", crate::complex::ctan);
    ctx.func("sin_d", sin_d);
    ctx.func("cos_d", cos_d);
    ctx.func("tan_d", tan_d);
    ctx.func("asin_d", asin_d);
    ctx.func("acos_d", acos_d);
    ctx.func("atan_d", atan_d);
    ctx.func2_real("atan2_d", |y: f64, x: f64| y.atan2(x) * 180.0 / PI);
    // 余函数(复数)
    ctx.func("sec", |z: Cmplx| Cmplx::real(1.0) / crate::complex::ccos(z));
    ctx.func("csc", |z: Cmplx| Cmplx::real(1.0) / crate::complex::csin(z));
    ctx.func("cot", |z: Cmplx| {
        crate::complex::ccos(z) / crate::complex::csin(z)
    });
    ctx.func("sech", |z: Cmplx| {
        Cmplx::real(1.0) / crate::complex::ccosh(z)
    });
    ctx.func("csch", |z: Cmplx| {
        Cmplx::real(1.0) / crate::complex::csinh(z)
    });
    ctx.func("coth", |z: Cmplx| {
        crate::complex::ccosh(z) / crate::complex::csinh(z)
    });
    // 组合/排列/Γ 函数(实数)
    ctx.func2_real("C", comb);
    ctx.func2_real("A", perm);
    ctx.func_real("gamma", gamma_lanczos);
    // 取整/舍入补充(实数)
    ctx.func_real("frac", |x: f64| x - x.floor());
    ctx.func2_real("round", |x: f64, n: f64| {
        if n.fract() != 0.0 || n.abs() > 300.0 {
            f64::NAN
        } else {
            let p = 10f64.powi(n as i32);
            (x * p).round() / p
        }
    });
    // 统计(变参实数)
    ctx.variadic_real("mean", 1, |a| a.iter().sum::<f64>() / a.len() as f64);
    ctx.variadic_real("var", 1, |a| {
        let n = a.len() as f64;
        let m = a.iter().sum::<f64>() / n;
        a.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / n
    });
    ctx.variadic_real("std", 1, |a| {
        let n = a.len() as f64;
        let m = a.iter().sum::<f64>() / n;
        (a.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / n).sqrt()
    });
    // 常量
    ctx.var("phi", Cmplx::real((1.0 + 5.0f64.sqrt()) / 2.0));
    ctx.var("g", Cmplx::real(9.80665));
    ctx.var("ln2", Cmplx::real(2.0f64.ln()));
    ctx.var("ln10", Cmplx::real(10.0f64.ln()));
    ctx.var("sqrt2", Cmplx::real(2.0f64.sqrt()));
    ctx.var("euler", Cmplx::real(0.5772156649015329));
    ctx
}

/// 组合数 C(n,k) = Π_{i=1..k} (n-k+i)/i(乘法公式避免中间阶乘溢出)。
fn comb(n: f64, k: f64) -> f64 {
    if n < 0.0 || k < 0.0 || n.fract() != 0.0 || k.fract() != 0.0 {
        return f64::NAN;
    }
    if k > n {
        return 0.0;
    }
    if k > 100_000.0 {
        return f64::NAN;
    }
    let k = k.min(n - k); // C(n,k) = C(n,n-k)
    let mut r = 1.0;
    for i in 1..=(k as u64) {
        r *= (n - k + i as f64) / i as f64;
    }
    r
}

/// 排列数 A(n,k) = Π_{i=0..k-1} (n-i)。
fn perm(n: f64, k: f64) -> f64 {
    if n < 0.0 || k < 0.0 || n.fract() != 0.0 || k.fract() != 0.0 {
        return f64::NAN;
    }
    if k > n {
        return 0.0;
    }
    if k > 100_000.0 {
        return f64::NAN;
    }
    let mut r = 1.0;
    for i in 0..(k as u64) {
        r *= n - i as f64;
    }
    r
}

/// Γ 函数(Lanczos 近似, 实数)。gamma(n) = (n-1)!, 负整数为极点。
fn gamma_lanczos(x: f64) -> f64 {
    if x <= 0.0 && x.fract() == 0.0 {
        return f64::NAN;
    }
    if x < 0.5 {
        // 反射公式: Γ(x) = π / (sin(πx)·Γ(1-x))
        return PI / ((PI * x).sin() * gamma_lanczos(1.0 - x));
    }
    const G: f64 = 7.0;
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.5203681218851,
        -1259.1392167224028,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507343278686905,
        -0.13857109526572012,
        9.984_369_578_019_572e-6,
        1.5056327351493116e-7,
    ];
    let x = x - 1.0;
    let mut a = C[0];
    for (i, c) in C.iter().enumerate().skip(1) {
        a += c / (x + i as f64);
    }
    let t = x + G + 0.5;
    (2.0 * PI).sqrt() * t.powf(x + 0.5) * (-t).exp() * a
}

// 上下文是纯函数表, 每次求值都重建成本高(级数/积分逐项求值时尤其明显),
// 用线程局部缓存。
thread_local! {
    static BASE_CTX: ExprContext = base_context();
}

fn eval_str(s: &str) -> Result<Cmplx, String> {
    BASE_CTX.with(|ctx| eval_with_context(s, ctx))
}

/// 对一个"参数片段"做完整预处理并求值(片段内允许嵌套复合结构)。
fn eval_plain(s: &str, degree: bool, funcs: &[UserFunc], depth: usize) -> Result<Cmplx, String> {
    let p = preprocess(s, degree, funcs, depth)?;
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
fn subst_var(s: &str, var: &str, val: Cmplx) -> String {
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

/// 级数收敛阈值: 连续若干项小于该量级即认为收敛。
const SERIES_TOL: f64 = 1e-12;
const SERIES_QUIET: u32 = 3;
const SERIES_MAX_TERMS: f64 = 1_000_000.0;

fn eval_sum(
    args: &[String],
    degree: bool,
    funcs: &[UserFunc],
    depth: usize,
) -> Result<Cmplx, String> {
    if args.len() != 4 {
        return Err("Σ 需要4个参数: Σ(变量, 起始, 结束, 表达式)".into());
    }
    let var = args[0].trim();
    validate_var(var)?;
    let start = eval_plain(&args[1], degree, funcs, depth + 1)?;
    let end = eval_plain(&args[2], degree, funcs, depth + 1)?;
    if !start.is_real() || !end.is_real() {
        return Err("Σ 的起始/结束必须是实数".into());
    }
    if start.re.is_infinite() {
        return Err("Σ 的起始不能是无穷".into());
    }
    let mut k = start.re.ceil();
    // 无穷级数: 结束为 +∞
    if end.re.is_infinite() {
        if end.re < 0.0 {
            return Err("Σ 的结束不能是负无穷".into());
        }
        let mut total = Cmplx::real(0.0);
        let mut quiet = 0u32;
        loop {
            let term = subst_var(&args[3], var, Cmplx::real(k));
            let pp = preprocess(&term, degree, funcs, depth + 1)?;
            let t = eval_str(&pp).map_err(|e| format!("求和项错误 (k={k}): {e}"))?;
            total = total + t;
            if t.abs() <= SERIES_TOL * (1.0 + total.abs()) {
                quiet += 1;
                if quiet >= SERIES_QUIET {
                    return Ok(total);
                }
            } else {
                quiet = 0;
            }
            k += 1.0;
            if k - start.re.ceil() > SERIES_MAX_TERMS {
                return Err("级数未收敛 (项数超过 1,000,000)".into());
            }
        }
    }
    let stop = end.re.floor();
    if k > stop {
        return Ok(Cmplx::real(0.0));
    }
    if stop - k > SERIES_MAX_TERMS {
        return Err("求和项数过多 (上限 1,000,000)".into());
    }
    let mut total = Cmplx::real(0.0);
    while k <= stop {
        let term = subst_var(&args[3], var, Cmplx::real(k));
        let pp = preprocess(&term, degree, funcs, depth + 1)?;
        let t = eval_str(&pp).map_err(|e| format!("求和项错误 (k={k}): {e}"))?;
        total = total + t;
        k += 1.0;
    }
    Ok(total)
}

fn simpson_step(
    f: &mut impl FnMut(f64) -> Result<Cmplx, String>,
    a: f64,
    b: f64,
    n: usize,
) -> Result<Cmplx, String> {
    let h = (b - a) / n as f64;
    let mut s = f(a)? + f(b)?;
    for i in 1..n {
        let x = a + i as f64 * h;
        let w = if i % 2 == 1 { 4.0 } else { 2.0 };
        s = s + f(x)? * w;
    }
    Ok(s * (h / 3.0))
}

fn adaptive_simpson(
    mut f: impl FnMut(f64) -> Result<Cmplx, String>,
    a: f64,
    b: f64,
) -> Result<Cmplx, String> {
    if a == b {
        return Ok(Cmplx::real(0.0));
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

/// 瑕积分: 截断策略——有限区间上自适应积分, 区间宽度翻倍直至相邻结果差小于容差。
fn eval_int(
    args: &[String],
    degree: bool,
    funcs: &[UserFunc],
    depth: usize,
) -> Result<Cmplx, String> {
    if args.len() != 4 {
        return Err("∫ 需要4个参数: ∫(变量, 下限, 上限, 表达式)".into());
    }
    let var = args[0].trim();
    validate_var(var)?;
    let a = eval_plain(&args[1], degree, funcs, depth + 1)?;
    let b = eval_plain(&args[2], degree, funcs, depth + 1)?;
    if !a.is_real() || !b.is_real() {
        return Err("∫ 的上下限必须是实数".into());
    }
    let a = a.re;
    let b = b.re;
    if a.is_infinite() && b.is_infinite() {
        return Err("∫ 的上下限不能同时为无穷".into());
    }
    let expr = args[3].clone();
    let mut f = move |x: f64| -> Result<Cmplx, String> {
        let s = subst_var(&expr, var, Cmplx::real(x));
        let pp = preprocess(&s, degree, funcs, depth + 1)?;
        eval_str(&pp)
    };
    if !a.is_infinite() && !b.is_infinite() {
        return adaptive_simpson(f, a, b);
    }
    // 单侧无穷: 环带累加——每段 [x, 2x] 单独自适应积分(宽区间整体辛普森
    // 收敛极慢), 直到新增环带小于容差(被积函数在无穷远处衰减)。
    // 下限为 -∞ 时从有限端向左累加得到 ∫_b^(-∞), 结果需取负。
    let negate = a.is_infinite();
    let (lo, dir) = if b.is_infinite() { (a, 1.0) } else { (b, -1.0) };
    let mut width = 1.0;
    let mut hi = lo + dir * width;
    let mut total = adaptive_simpson(&mut f, lo, hi)?;
    for _ in 0..64 {
        let piece = adaptive_simpson(&mut f, hi, hi + dir * width)?;
        total = total + piece;
        if piece.abs() <= 1e-10 * (1.0 + total.abs()) {
            return Ok(if negate { -total } else { total });
        }
        hi += dir * width;
        width *= 2.0;
    }
    Err("积分未收敛 (被积函数在无穷远处衰减过慢或振荡)".into())
}

fn eval_deriv(
    args: &[String],
    degree: bool,
    funcs: &[UserFunc],
    depth: usize,
) -> Result<Cmplx, String> {
    if args.len() != 3 {
        return Err("deriv 需要3个参数: deriv(表达式, 变量, 取值点x0)".into());
    }
    let var = args[1].trim();
    validate_var(var)?;
    let x0 = eval_plain(&args[2], degree, funcs, depth + 1)?;
    let expr = args[0].clone();
    let h = 1e-4;
    let f = |x: Cmplx| -> Result<Cmplx, String> {
        let s = subst_var(&expr, var, x);
        let pp = preprocess(&s, degree, funcs, depth + 1)?;
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

/// 表达式内替换使用的紧凑数值(保证求值器可解析)。
fn fmt_num_real(v: f64) -> String {
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

/// 表达式内替换使用的数值(复数用括号包裹, 保证优先级与可解析性)。
pub fn fmt_num(v: Cmplx) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_real() {
        return fmt_num_real(v.re);
    }
    let re = fmt_num_real(v.re);
    let im = fmt_num_real(v.im.abs());
    let sign = if v.im < 0.0 { "-" } else { "+" };
    let im_part = if im == "1" { String::new() } else { im };
    format!("({re}{sign}{im_part}i)")
}

/// 结果展示用的实数格式。
fn fmt_result_real(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v.is_sign_negative() {
            "-∞".into()
        } else {
            "∞".into()
        };
    }
    if v == 0.0 {
        return "0".into();
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

/// 结果展示用数值(复数显示为 a+bi, 虚部为 0 时显示实数)。
pub fn fmt_result(v: Cmplx) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_real() {
        return fmt_result_real(v.re);
    }
    if v.re.is_infinite() || v.im.is_infinite() {
        return "∞".into();
    }
    let re = fmt_result_real(v.re);
    let im = fmt_result_real(v.im.abs());
    let sign = if v.im < 0.0 { "-" } else { "+" };
    let im_part = if im == "1" { String::new() } else { im };
    if v.re == 0.0 {
        format!("{}{}i", if v.im < 0.0 { "-" } else { "" }, im_part)
    } else {
        format!("{re}{sign}{im_part}i")
    }
}

// ---------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;
    use crate::complex::{
        cacos, casin, catan, ccosh, cos_snap, csinh, ctanh, tan_snap, trig_snap_arg,
    };

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn ok(e: &str) -> f64 {
        let z = Evaluator::new(false)
            .evaluate(e)
            .unwrap_or_else(|_| panic!("应求值成功: {e}"));
        assert!(z.is_real(), "{e} 应为实数, 实际: {z:?}");
        z.re
    }
    fn deg(e: &str) -> f64 {
        let z = Evaluator::new(true).evaluate(e).expect("应求值成功");
        assert!(z.is_real(), "{e} 应为实数, 实际: {z:?}");
        z.re
    }
    fn cz(e: &str) -> Cmplx {
        Evaluator::new(false)
            .evaluate(e)
            .unwrap_or_else(|_| panic!("应求值成功: {e}"))
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
    fn trig_exact_values() {
        assert_eq!(deg("sin(180)"), 0.0);
        assert_eq!(deg("cos(180)"), -1.0);
        assert_eq!(deg("tan(180)"), 0.0);
        assert_eq!(deg("sin(90)"), 1.0);
        assert_eq!(deg("cos(90)"), 0.0);
        assert_eq!(deg("sin(270)"), -1.0);
        assert_eq!(deg("sin(360)"), 0.0);
        assert_eq!(deg("tan(90)"), f64::INFINITY);
        assert_eq!(deg("tan(270)"), f64::INFINITY);
        assert_eq!(deg("tan(-90)"), f64::NEG_INFINITY);
        assert_eq!(deg("tan(-270)"), f64::NEG_INFINITY);
        assert_eq!(deg("sin(-90)"), -1.0);
        assert_eq!(deg("cos(-90)"), 0.0);
        assert_eq!(deg("sin(3600000)"), 0.0, "大角度经 360° 归约后仍应精确");
        assert_eq!(deg("cos(3600000)"), 1.0);
        assert_eq!(deg("tan(3600000)"), 0.0);
        assert_eq!(deg("sin(-3600000)"), 0.0);
        assert_eq!(ok("sin(pi)"), 0.0);
        assert_eq!(ok("cos(pi)"), -1.0);
        assert_eq!(ok("tan(pi)"), 0.0);
        assert_eq!(ok("sin(pi/2)"), 1.0);
        assert_eq!(ok("cos(pi/2)"), 0.0);
        assert_eq!(ok("tan(pi/2)"), f64::INFINITY);
        assert_eq!(ok("tan(-pi/2)"), f64::NEG_INFINITY);
        assert_eq!(ok("tan(3*pi/2)"), f64::INFINITY);
        assert_eq!(ok("tan(-3*pi/2)"), f64::NEG_INFINITY);
        assert_eq!(ok("sin(-pi)"), 0.0);
        assert_eq!(ok("sin(2*pi)"), 0.0);
        assert_eq!(ok("cos(3*pi/2)"), 0.0);
        assert_eq!(ok("tan(3*pi)"), 0.0);
        assert_eq!(ok("tan(pi/2-1e-13)"), f64::INFINITY, "左侧趋近 π/2 → +∞");
        assert_eq!(
            ok("tan(pi/2+1e-13)"),
            f64::NEG_INFINITY,
            "右侧趋近 π/2 → -∞"
        );
        assert_eq!(ok("tan(-pi/2-1e-13)"), f64::INFINITY, "左侧趋近 -π/2 → +∞");
        assert_eq!(
            ok("tan(-pi/2+1e-13)"),
            f64::NEG_INFINITY,
            "右侧趋近 -π/2 → -∞"
        );
        assert!(approx(ok("sin(pi/6)"), 0.5));
        assert!(approx(ok("cos(pi/3)"), 0.5));
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
    fn trig_full_period_rad() {
        // sin/cos/tan 全周期 [-π, π] 扫描: 未矫正点必须与标准库一致;
        // 矫正点必须位于 π/2 整数倍附近且值为精确三角函数值。
        let n = 10_000;
        for i in 0..=n {
            let x = -PI + 2.0 * PI * (i as f64) / (n as f64);
            let (rs, rc, rt) = (x.sin(), x.cos(), x.tan());
            let (s, c, t) = (sin_snap(x), cos_snap(x), tan_snap(x));
            if (s - rs).abs() > 1e-12 {
                assert!(
                    trig_snap_arg(x).is_some() && (s == 0.0 || s == 1.0 || s == -1.0),
                    "x={x}: 矫正值 {s} 不合法"
                );
            }
            if (c - rc).abs() > 1e-12 {
                assert!(
                    trig_snap_arg(x).is_some() && (c == 0.0 || c == 1.0 || c == -1.0),
                    "x={x}: 矫正值 {c} 不合法"
                );
            }
            if (t - rt).abs() > 1e-12 {
                assert!(trig_snap_arg(x).is_some(), "x={x}: 不该矫正");
                if t.is_infinite() {
                    assert_eq!(
                        t.is_sign_positive(),
                        rt.is_sign_positive(),
                        "x={x}: ∞ 符号不符"
                    );
                } else {
                    assert_eq!(t, 0.0, "x={x}: 矫正值 {t} 不合法");
                }
            }
        }
    }

    #[test]
    fn trig_full_period_deg() {
        // DEG 模式多周期扫描 [-720°, 720°], 覆盖负角与跨周期角
        let mut x = -720.0;
        while x <= 720.0 {
            let r = x * PI / 180.0;
            let z = Cmplx::real(x);
            let (s, c) = (sin_d(z), cos_d(z));
            assert!((s - r.sin()).abs() < 1e-12, "x={x}°: sin 不一致");
            assert!((c - r.cos()).abs() < 1e-12, "x={x}°: cos 不一致");
            let t = tan_d(z);
            if !t.is_real() {
                panic!("x={x}°: tan 应为实数");
            }
            let t = t.re;
            if t.is_infinite() {
                assert_eq!(
                    t.is_sign_positive(),
                    r.tan().is_sign_positive(),
                    "x={x}°: ∞ 符号不符"
                );
            } else {
                assert!((t - r.tan()).abs() < 1e-12, "x={x}°: tan 不一致");
            }
            x += 0.5;
        }
    }

    #[test]
    fn arc_and_hyperbolic_full_domain() {
        // 反三角函数/双曲函数在定义域内采样, 与标准库逐点一致
        let n = 2_000;
        for i in 0..=n {
            let x = -1.0 + 2.0 * (i as f64) / (n as f64);
            let z = Cmplx::real(x);
            assert_eq!(eval_str(&format!("asin({x})")).unwrap(), casin(z), "x={x}");
            assert_eq!(eval_str(&format!("acos({x})")).unwrap(), cacos(z), "x={x}");
            assert_eq!(eval_str(&format!("atan({x})")).unwrap(), catan(z), "x={x}");
            assert_eq!(eval_str(&format!("sinh({x})")).unwrap(), csinh(z), "x={x}");
            assert_eq!(eval_str(&format!("cosh({x})")).unwrap(), ccosh(z), "x={x}");
            assert_eq!(eval_str(&format!("tanh({x})")).unwrap(), ctanh(z), "x={x}");
        }
        // 端点与无穷极限
        assert_eq!(ok("asin(1)"), PI / 2.0);
        assert_eq!(ok("asin(-1)"), -PI / 2.0);
        assert_eq!(ok("acos(1)"), 0.0);
        assert_eq!(ok("acos(-1)"), PI);
        assert_eq!(ok("atan(1e999)"), PI / 2.0);
        assert_eq!(ok("atan(-1e999)"), -PI / 2.0);
        assert_eq!(ok("asinh(1e999)"), f64::INFINITY);
        assert_eq!(ok("cosh(1e999)"), f64::INFINITY);
        assert_eq!(ok("tanh(1e999)"), 1.0);
        assert_eq!(ok("tanh(-1e999)"), -1.0);
        assert_eq!(ok("cosh(0)"), 1.0);
        assert_eq!(ok("tanh(0)"), 0.0);
        assert!(
            approx(ok("cosh(1)^2-sinh(1)^2"), 1.0),
            "cosh²−sinh²=1 恒等式"
        );
        assert!(
            approx(ok("tanh(1)"), ok("sinh(1)/cosh(1)")),
            "tanh=sinh/cosh 恒等式"
        );
        // 复数域上定义域自然扩展: 越界实参得到主分支复数值
        let approx_z = |a: Cmplx, b: Cmplx| (a - b).abs() < 1e-9;
        assert!(
            approx_z(cz("asin(2)"), Cmplx::new(PI / 2.0, -1.3169578969)),
            "asin(2) 复数主分支"
        );
        assert!(
            approx_z(cz("asin(-2)"), Cmplx::new(-PI / 2.0, 1.3169578969)),
            "asin(-2) 复数主分支"
        );
        err("asin(1e999)", false); // 超出 f64 表示范围 → NaN 报错
        assert!(approx_z(cz("acos(2)"), Cmplx::new(0.0, -1.3169578969)));
        assert!(approx_z(cz("acos(-2)"), Cmplx::new(PI, 1.3169578969)));
        assert!(approx_z(cz("acosh(0.5)"), Cmplx::new(0.0, PI / 3.0)));
        assert!(approx_z(cz("acosh(0)"), Cmplx::new(0.0, PI / 2.0)));
        assert!(approx_z(
            cz("atanh(2)"),
            Cmplx::new(0.5 * 3.0f64.ln(), -PI / 2.0)
        ));
        assert!(approx_z(
            cz("atanh(-2)"),
            Cmplx::new(-0.5 * 3.0f64.ln(), PI / 2.0)
        ));
        // DEG 反三角关键角
        assert_eq!(deg("asin(1)"), 90.0);
        assert_eq!(deg("acos(-1)"), 180.0);
        assert_eq!(deg("atan(1)"), 45.0);
        assert_eq!(deg("atan2(1,1)"), 45.0);
        assert_eq!(deg("atan(1/0)"), 90.0);
        assert!(approx(deg("asin(0.5)"), 30.0));
        assert!(approx(deg("acos(0.5)"), 60.0));
    }

    // ------------------------------------------------------------ 复数计算

    #[test]
    fn complex_arithmetic() {
        assert_eq!(cz("i^2"), Cmplx::real(-1.0));
        assert_eq!(cz("2i"), Cmplx::new(0.0, 2.0), "隐式乘法 2i");
        assert_eq!(
            cz("(1+2i)*(3-4i)"),
            Cmplx::new(11.0, 2.0),
            "隐式乘法复数运算"
        );
        assert_eq!(cz("sqrt(-1)"), Cmplx::I);
        assert_eq!(cz("sqrt(-4)"), Cmplx::new(0.0, 2.0));
        assert_eq!(cz("abs(3+4i)"), Cmplx::real(5.0));
        assert_eq!(cz("ln(-1)"), Cmplx::new(0.0, PI));
        assert_eq!(cz("e^(i*pi)"), Cmplx::real(-1.0), "e^(iπ) = -1 精确");
        assert_eq!(cz("e^(i*pi/2)"), Cmplx::I, "e^(iπ/2) = i 精确");
        assert_eq!(cz("conj(3-4i)"), Cmplx::new(3.0, 4.0));
        assert_eq!(cz("arg(1+i)"), Cmplx::real(PI / 4.0));
        assert_eq!(cz("re(2+3i)"), Cmplx::real(2.0));
        assert_eq!(cz("im(2+3i)"), Cmplx::real(3.0));
        assert_eq!(cz("1/(2i)"), Cmplx::new(0.0, -0.5));
        // 实数精确性保持
        assert_eq!(ok("2^10"), 1024.0);
        assert_eq!(ok("1/0"), f64::INFINITY);
    }

    #[test]
    fn complex_funcs() {
        let approx_z = |a: Cmplx, b: Cmplx| (a - b).abs() < 1e-9;
        assert!(approx_z(cz("sin(i)"), Cmplx::new(0.0, 1.0f64.sinh())));
        assert!(approx_z(cz("cos(i)"), Cmplx::real(1.0f64.cosh())));
        assert!(approx_z(cz("tan(1+i)"), ctan(Cmplx::new(1.0, 1.0))));
        assert!(approx_z(cz("sinh(i)"), Cmplx::new(0.0, 1.0f64.sin())));
        assert!(approx_z(cz("cosh(i)"), Cmplx::real(1.0f64.cos())));
        assert!(approx_z(cz("tanh(i)"), Cmplx::new(0.0, 1.0f64.tan())));
        assert!(approx_z(
            cz("(1+i)^(1+i)"),
            (Cmplx::new(1.0, 1.0)).pow(Cmplx::new(1.0, 1.0))
        ));
        // 实数专属函数对复数报错
        err("fact(2+i)", false);
        err("floor(1.5+i)", false);
        err("max(1,2+i)", false);
        err("(1+i)%2", false);
        err("C(3,2+i)", false);
        // DEG 模式复数
        let degz = |e: &str| Evaluator::new(true).evaluate(e).expect("应求值成功");
        assert!(approx_z(
            degz("sin_d(90+30i)"),
            csin(Cmplx::new(90.0, 30.0) * (PI / 180.0))
        ));
        assert_eq!(degz("sin_d(90)"), Cmplx::real(1.0));
        // 自定义函数复数体
        let funcs = [UserFunc {
            name: "f_1".into(),
            body: "x^2+i".into(),
        }];
        assert_eq!(feval_c(&funcs, "f_1(2)"), Cmplx::new(4.0, 1.0));
        // Σ/∫/deriv 复数被积
        assert!(approx_z(cz("Σ(k,1,3,k*i)"), Cmplx::new(0.0, 6.0)));
        assert!(approx_z(cz("∫(x,0,1,x+i)"), Cmplx::new(0.5, 1.0)));
        assert!(approx_z(cz("deriv(x^3+i*x,x,2)"), Cmplx::new(12.0, 1.0)));
    }

    fn feval_c(funcs: &[UserFunc], expr: &str) -> Cmplx {
        let mut e = Evaluator::new(false);
        e.funcs = funcs.to_vec();
        e.evaluate(expr)
            .unwrap_or_else(|_| panic!("应求值成功: {expr}"))
    }

    // ------------------------------------------------------------ 新函数与常量

    #[test]
    fn comb_and_perm() {
        assert_eq!(ok("C(5,2)"), 10.0);
        assert_eq!(ok("C(5,3)"), 10.0, "C(n,k)=C(n,n-k)");
        assert_eq!(ok("C(10,0)"), 1.0);
        assert_eq!(ok("C(10,10)"), 1.0);
        assert_eq!(ok("C(3,5)"), 0.0, "k>n 时为 0");
        let c = ok("C(200,100)");
        assert!(c > 9.0e58 && c < 9.1e58, "大组合数不应中间溢出: {c}");
        assert_eq!(ok("A(5,2)"), 20.0);
        assert_eq!(ok("A(5,5)"), 120.0);
        assert_eq!(ok("A(5,6)"), 0.0);
        assert_eq!(ok("A(4,0)"), 1.0);
        err("C(2.5,2)", false);
        err("C(-1,2)", false);
        err("A(2.5,2)", false);
    }

    #[test]
    fn gamma_function() {
        assert!(approx(ok("gamma(5)"), 24.0), "gamma(5) = 4! = 24");
        assert!(approx(ok("gamma(1)"), 1.0));
        assert!(approx(ok("gamma(0.5)"), PI.sqrt()), "gamma(0.5) = √π");
        assert!(approx(ok("gamma(1.5)"), PI.sqrt() / 2.0));
        err("gamma(0)", false);
        err("gamma(-1)", false);
        err("gamma(-2)", false);
        err("gamma(2+i)", false); // 复数参数报错
    }

    #[test]
    fn reciprocal_funcs() {
        assert!(approx(ok("sec(0)"), 1.0));
        assert!(approx(ok("csc(pi/2)"), 1.0));
        assert!(approx(ok("cot(pi/4)"), 1.0));
        assert!(approx(ok("sech(0)"), 1.0));
        assert!(approx(ok("csch(1)"), 1.0f64.sinh().recip()));
        assert!(approx(ok("coth(1)"), 1.0f64.tanh().recip()));
        assert!(approx(ok("sec(pi/3)"), 2.0));
        assert!(approx(ok("csc(pi/6)"), 2.0));
        // 复数余函数
        let approx_z = |a: Cmplx, b: Cmplx| (a - b).abs() < 1e-9;
        assert!(approx_z(cz("sec(i)"), Cmplx::real(1.0) / ccos(Cmplx::I)));
        assert!(approx_z(
            cz("cot(1+i)"),
            ccos(Cmplx::new(1.0, 1.0)) / csin(Cmplx::new(1.0, 1.0))
        ));
        assert_eq!(ok("1/sec(0)"), 1.0);
    }

    #[test]
    fn rounding_and_constants() {
        assert_eq!(ok("frac(3.75)"), 0.75);
        assert_eq!(ok("frac(-2.5)"), 0.5, "小数部分约定为 [0,1)");
        assert_eq!(ok("frac(4)"), 0.0);
        assert_eq!(ok("round(9.876,2)"), 9.88);
        assert_eq!(ok("round(3.145,2)"), 3.15);
        assert_eq!(ok("round(2.5,0)"), 3.0);
        assert_eq!(ok("round(1234.5678,3)"), 1234.568);
        err("round(3.14,2.5)", false);
        // 常量
        assert!(approx(ok("ln2"), 2.0f64.ln()));
        assert!(approx(ok("ln10"), 10.0f64.ln()));
        assert!(approx(ok("sqrt2"), 2.0f64.sqrt()));
        assert!(
            approx(ok("γ"), 0.5772156649015329),
            "γ 符号应映射为欧拉常数"
        );
        assert!(approx(ok("2γ"), 2.0 * 0.5772156649015329));
    }

    #[test]
    fn statistics() {
        assert_eq!(ok("mean(1,2,3,4)"), 2.5);
        assert_eq!(ok("mean(5)"), 5.0);
        assert_eq!(ok("var(1,2,3,4)"), 1.25, "总体方差");
        assert_eq!(ok("var(5)"), 0.0);
        assert!(approx(ok("std(1,2,3,4)"), 1.25f64.sqrt()));
        assert_eq!(ok("max(3,1,4,1,5)"), 5.0);
        assert_eq!(ok("min(3,1,4,1,5)"), 1.0);
        assert!(approx(
            ok("mean(1,2,3)+std(1,2,3)"),
            2.0 + (2.0f64 / 3.0).sqrt()
        ));
        // 统计/最值仅支持实数
        err("mean(1,2+i)", false);
        err("var(1,2+i)", false);
        err("max(1,2+i)", false);
    }

    // ------------------------------------------------------------ 无穷界

    #[test]
    fn infinite_series() {
        // 截断求和: 慢衰减级数(1/k^p)的截断误差约 1/k_max, 用相对容差
        let rel = |a: f64, b: f64| (a - b).abs() < 1e-4 * (1.0 + b.abs());
        assert!(rel(ok("Σ(k,1,∞,1/k^2)"), PI * PI / 6.0), "巴塞尔问题");
        assert!(rel(ok("Σ(k,1,∞,1/(k*(k+1)))"), 1.0));
        assert_eq!(ok("Σ(k,0,∞,1/2^k)"), 2.0, "几何级数应精确");
        // 交错级数误差 ≤ 下一项, 快速收敛
        assert!(approx(ok("Σ(k,1,∞,(-1)^(k+1)/k^2)"), PI * PI / 12.0));
        err("Σ(k,1,∞,1/k)", false); // 调和级数发散
        err("Σ(k,1,-∞,1/k^2)", false);
        err("Σ(k,∞,1,1)", false);
        err("Σ(k,1,∞,1)", false); // 常数项级数不收敛
        err("Σ(k,1,∞,k)", false);
    }

    #[test]
    fn improper_integrals() {
        assert!(approx(ok("∫(x,1,∞,1/x^2)"), 1.0));
        assert!(approx(ok("∫(x,0,∞,e^(-x))"), 1.0));
        assert!(approx(ok("∫(x,-∞,0,e^(x))"), 1.0));
        assert!(approx(ok("∫(x,1,∞,1/x^3)"), 0.5));
        err("∫(x,1,∞,1/x)", false); // 不收敛
        err("∫(x,-∞,∞,1)", false); // 双无穷
    }

    #[test]
    fn mod_and_abs() {
        assert_eq!(ok("mod(8,3)"), 2.0);
        assert_eq!(ok("mod(10,4)"), 2.0);
        assert_eq!(ok("mod(2.5,1)"), 0.5);
        assert!(approx(ok("mod(-8,3)"), -2.0));
        assert_eq!(ok("5+mod(8,3)"), 7.0);
        err("mod(8,0)", false);
        assert_eq!(ok("|-5|"), 5.0);
        assert_eq!(ok("|3-5|"), 2.0);
        assert_eq!(ok("|2|+|3|"), 5.0);
        assert_eq!(ok("2|3|"), 6.0);
        assert!(approx(ok("|sin(pi/2)|"), 1.0));
        assert_eq!(ok("abs(-5)"), 5.0);
        assert!(approx(ok("|√(16)|"), 4.0));
        assert_eq!(ok("Σ(k,1,3,|k-2|)"), 2.0);
    }

    #[test]
    fn golden_ratio_and_gravity() {
        let phi = (1.0 + 5.0f64.sqrt()) / 2.0;
        assert!(approx(ok("φ"), phi));
        assert!(approx(ok("2φ"), 2.0 * phi));
        assert!(approx(ok("φ^2"), phi + 1.0), "φ² = φ+1");
        assert_eq!(ok("g"), 9.80665);
        assert_eq!(ok("2g"), 19.6133);
        assert!(approx(ok("g/2"), 4.903325));
        assert!(approx(ok("φ+π"), phi + std::f64::consts::PI));
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
        assert_eq!(fmt_num(Cmplx::real(6.0)), "6");
        assert_eq!(fmt_num(Cmplx::real(-0.5)), "-0.5");
        assert_eq!(fmt_num(Cmplx::real(0.3333333333333333)), "0.3333333333");
        assert_eq!(fmt_num(Cmplx::new(1.0, 2.0)), "(1+2i)");
        assert_eq!(fmt_num(Cmplx::new(1.0, -2.0)), "(1-2i)");
        assert_eq!(fmt_num(Cmplx::new(1.0, 1.0)), "(1+i)");
        assert_eq!(fmt_result(Cmplx::real(f64::INFINITY)), "∞");
        assert_eq!(fmt_result(Cmplx::real(f64::NEG_INFINITY)), "-∞");
        assert_eq!(fmt_result(Cmplx::real(-0.0)), "0");
        assert_eq!(fmt_result(Cmplx::real(0.0)), "0");
        assert_eq!(fmt_result(Cmplx::real(-1.0)), "-1");
        assert_eq!(fmt_result(Cmplx::new(1.0, 2.0)), "1+2i");
        assert_eq!(fmt_result(Cmplx::new(1.0, -1.0)), "1-i");
        assert_eq!(fmt_result(Cmplx::new(0.0, 2.0)), "2i");
        assert_eq!(fmt_result(Cmplx::new(0.0, 1.0)), "i");
        assert_eq!(fmt_result(Cmplx::new(0.0, -1.0)), "-i");
        assert_eq!(fmt_result(Cmplx::new(-1.0, 1.0)), "-1+i");
    }

    // ------------------------------------------------------------ 自定义函数

    fn feval(funcs: &[UserFunc], expr: &str) -> f64 {
        let mut e = Evaluator::new(false);
        e.funcs = funcs.to_vec();
        let z = e.evaluate(expr).expect("应求值成功");
        assert!(z.is_real(), "{expr} 应为实数: {z:?}");
        z.re
    }
    fn fdeg(funcs: &[UserFunc], expr: &str) -> f64 {
        let mut e = Evaluator::new(true);
        e.funcs = funcs.to_vec();
        let z = e.evaluate(expr).expect("应求值成功");
        assert!(z.is_real(), "{expr} 应为实数: {z:?}");
        z.re
    }
    fn ferr(funcs: &[UserFunc], expr: &str) {
        let mut e = Evaluator::new(false);
        e.funcs = funcs.to_vec();
        assert!(e.evaluate(expr).is_err(), "应报错: {expr}");
    }
    fn f1(body: &str) -> UserFunc {
        UserFunc {
            name: "f_1".into(),
            body: body.into(),
        }
    }

    #[test]
    fn user_funcs_basic() {
        assert_eq!(feval(&[f1("x+1")], "f_1(1)"), 2.0);
        assert_eq!(feval(&[f1("x+1")], "f_1(2*3)"), 7.0, "参数应先求值再代入");
        assert_eq!(feval(&[f1("x^2")], "f_1(3)"), 9.0);
        assert_eq!(
            feval(&[f1("x+1")], "f_1(2)*3"),
            9.0,
            "函数体应整体参与外部运算"
        );
        assert_eq!(
            feval(&[f1("x+1")], "f_1(2)3"),
            9.0,
            "调用后跟数字应隐式相乘"
        );
        assert_eq!(
            feval(&[f1("x+1")], "2f_1(3)"),
            8.0,
            "数字后跟调用应隐式相乘"
        );
        assert_eq!(feval(&[f1("x+1")], "f_1(2)-f_1(1)"), 1.0);
        assert!(approx(feval(&[f1("x")], "2f_1(3)"), 6.0));
    }

    #[test]
    fn user_funcs_nested_and_composite() {
        // 函数体内使用其他自定义函数
        let funcs = vec![
            UserFunc {
                name: "f_1".into(),
                body: "f_2(x)+1".into(),
            },
            UserFunc {
                name: "f_2".into(),
                body: "x*2".into(),
            },
        ];
        assert_eq!(feval(&funcs, "f_1(3)"), 7.0);
        assert_eq!(feval(&funcs, "f_2(f_1(1))"), 6.0);
        // 函数体含 Σ/∫/deriv
        assert_eq!(feval(&[f1("Σ(k,1,3,k*x)")], "f_1(2)"), 12.0);
        assert!(approx(feval(&[f1("∫(t,0,x,t^2)")], "f_1(3)"), 9.0));
        assert!(approx(feval(&[f1("deriv(t^2,t,x)")], "f_1(3)"), 6.0));
        // 在 Σ 项/deriv 中使用自定义函数
        assert_eq!(feval(&[f1("x+1")], "Σ(k,1,3,f_1(k))"), 9.0);
        assert!(approx(feval(&[f1("x^3")], "deriv(f_1(x),x,2)"), 12.0));
    }

    #[test]
    fn user_funcs_degree_mode() {
        assert_eq!(feval(&[f1("sin(x)")], "f_1(pi/2)"), 1.0);
        assert_eq!(
            fdeg(&[f1("sin(x)")], "f_1(90)"),
            1.0,
            "DEG 模式下函数体应跟随角度模式"
        );
        assert_eq!(fdeg(&[f1("cos(x)")], "f_1(180)"), -1.0);
    }

    #[test]
    fn user_funcs_name_matching() {
        let funcs = vec![
            f1("x+1"),
            UserFunc {
                name: "f_10".into(),
                body: "x*2".into(),
            },
        ];
        assert_eq!(feval(&funcs, "f_10(3)"), 6.0, "f_10 不应误匹配 f_1");
        assert_eq!(feval(&funcs, "f_1(3)"), 4.0);
        assert_eq!(feval(&funcs, "f_1(3)+f_10(3)"), 10.0);
    }

    #[test]
    fn user_funcs_errors() {
        ferr(&[f1("f_1(x)")], "f_1(2)"); // 自引用 → 嵌套过深
        ferr(&[f1("x+1")], "f_1()");
        ferr(&[f1("x+1")], "f_1(1,2)");
        ferr(&[f1("x+1")], "f_1(0/0)");
        ferr(&[f1("x+1")], "f_1(x+1)"); // 参数含未知变量
        ferr(&[f1("x+1")], "f_9(2)"); // 未定义函数
    }

    #[test]
    fn func_name_referenced_boundaries() {
        assert!(func_name_referenced("f_2(x)+1", "f_2"));
        assert!(func_name_referenced("x+f_2", "f_2"));
        assert!(func_name_referenced("f_2", "f_2"));
        assert!(
            !func_name_referenced("f_20(1)", "f_2"),
            "f_20 不应算引用 f_2"
        );
        assert!(!func_name_referenced("f_2x", "f_2"));
        assert!(!func_name_referenced("x2", "f_2"));
    }
}

//! Parameter equations: numbers with optional units, parameter names, `+ - * / ^`, parentheses
//! and a few functions. Lengths are in millimetres and angles in degrees, as they are shown.

use std::collections::BTreeMap;

/// A parsed equation.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Num(f64),
    Name(String),
    Neg(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

/// Longest equation accepted (hostile-input cap).
const MAX_LEN: usize = 1_000;
/// Deepest nesting accepted.
const MAX_DEPTH: usize = 64;

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Sym(char),
}

fn unit_factor(u: &str) -> Option<f64> {
    Some(match u {
        "mm" => 1.0,
        "cm" => 10.0,
        "m" => 1000.0,
        "in" => 25.4,
        "ft" => 304.8,
        "deg" => 1.0,
        "rad" => 180.0 / std::f64::consts::PI,
        "ul" => 1.0,
        _ => return None,
    })
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    if s.len() > MAX_LEN {
        return Err("the equation is too long".into());
    }
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || (c == '.' && cs.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let start = i;
            while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.') {
                i += 1;
            }
            // Exponent: 1e3, 2.5E-2.
            if i < cs.len() && (cs[i] == 'e' || cs[i] == 'E') {
                let mut j = i + 1;
                if j < cs.len() && (cs[j] == '+' || cs[j] == '-') {
                    j += 1;
                }
                if j < cs.len() && cs[j].is_ascii_digit() {
                    i = j;
                    while i < cs.len() && cs[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let text: String = cs[start..i].iter().collect();
            out.push(Tok::Num(text.parse().map_err(|_| format!("`{text}` is not a number"))?));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
                i += 1;
            }
            out.push(Tok::Ident(cs[start..i].iter().collect()));
        } else if "+-*/^(),".contains(c) {
            out.push(Tok::Sym(c));
            i += 1;
        } else {
            return Err(format!("unexpected `{c}`"));
        }
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    at: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.at).cloned();
        self.at += 1;
        t
    }
    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(&Tok::Sym(c)) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn deeper(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH { Err("the equation is nested too deeply".into()) } else { Ok(()) }
    }

    // sum := term (('+' | '-') term)*
    fn sum(&mut self) -> Result<Expr, String> {
        let mut e = self.term()?;
        loop {
            let op = if self.eat('+') {
                Op::Add
            } else if self.eat('-') {
                Op::Sub
            } else {
                return Ok(e);
            };
            e = Expr::Bin(op, Box::new(e), Box::new(self.term()?));
        }
    }

    // term := unary (('*' | '/') unary | implicit-multiplication)*
    fn term(&mut self) -> Result<Expr, String> {
        let mut e = self.unary()?;
        loop {
            let op = if self.eat('*') {
                Op::Mul
            } else if self.eat('/') {
                Op::Div
            } else {
                return Ok(e);
            };
            e = Expr::Bin(op, Box::new(e), Box::new(self.unary()?));
        }
    }

    // unary := '-' unary | '+' unary | power
    fn unary(&mut self) -> Result<Expr, String> {
        self.deeper()?;
        let e = if self.eat('-') {
            Expr::Neg(Box::new(self.unary()?))
        } else if self.eat('+') {
            self.unary()?
        } else {
            self.power()?
        };
        self.depth -= 1;
        Ok(e)
    }

    // power := atom ('^' unary)?   (right-associative)
    fn power(&mut self) -> Result<Expr, String> {
        let base = self.atom()?;
        if self.eat('^') { Ok(Expr::Bin(Op::Pow, Box::new(base), Box::new(self.unary()?))) } else { Ok(base) }
    }

    // atom := number unit? | name | name '(' args ')' | '(' sum ')' unit?
    fn atom(&mut self) -> Result<Expr, String> {
        let e = match self.next() {
            Some(Tok::Num(v)) => Expr::Num(v),
            Some(Tok::Ident(name)) => {
                if self.eat('(') {
                    let mut args = Vec::new();
                    if !self.eat(')') {
                        loop {
                            args.push(self.sum()?);
                            if self.eat(')') {
                                break;
                            }
                            if !self.eat(',') {
                                return Err(format!("`{name}(`: expected `,` or `)`"));
                            }
                        }
                    }
                    return Ok(Expr::Call(name, args));
                }
                match name.as_str() {
                    "PI" | "pi" => Expr::Num(std::f64::consts::PI),
                    "E" => Expr::Num(std::f64::consts::E),
                    _ => Expr::Name(name),
                }
            }
            Some(Tok::Sym('(')) => {
                self.deeper()?;
                let e = self.sum()?;
                self.depth -= 1;
                if !self.eat(')') {
                    return Err("a `(` is not closed".into());
                }
                e
            }
            Some(Tok::Sym(c)) => return Err(format!("unexpected `{c}`")),
            None => return Err("the equation ends too early".into()),
        };
        // A unit right after a value: `10 mm`, `(a + 2) in`, `PI rad`.
        if let Some(Tok::Ident(u)) = self.peek()
            && let Some(f) = unit_factor(u)
        {
            self.at += 1;
            return Ok(if (f - 1.0).abs() < f64::EPSILON { e } else { Expr::Bin(Op::Mul, Box::new(e), Box::new(Expr::Num(f))) });
        }
        Ok(e)
    }
}

/// Parses an equation.
pub fn parse(s: &str) -> Result<Expr, String> {
    let toks = lex(s)?;
    if toks.is_empty() {
        return Err("the equation is empty".into());
    }
    let mut p = Parser { toks, at: 0, depth: 0 };
    let e = p.sum()?;
    match p.peek() {
        None => Ok(e),
        Some(Tok::Num(v)) => Err(format!("unexpected `{v}`")),
        Some(Tok::Ident(n)) => Err(format!("unexpected `{n}` (an operator is missing, or it is not a unit)")),
        Some(Tok::Sym(c)) => Err(format!("unexpected `{c}`")),
    }
}

impl Expr {
    /// The parameter names it uses.
    pub fn names(&self, out: &mut Vec<String>) {
        match self {
            Expr::Num(_) => {}
            Expr::Name(n) => {
                if !out.contains(n) {
                    out.push(n.clone());
                }
            }
            Expr::Neg(e) => e.names(out),
            Expr::Bin(_, a, b) => {
                a.names(out);
                b.names(out);
            }
            Expr::Call(_, args) => args.iter().for_each(|a| a.names(out)),
        }
    }

    /// Its value, with parameter values from `env`.
    pub fn eval(&self, env: &BTreeMap<String, f64>) -> Result<f64, String> {
        let v = match self {
            Expr::Num(v) => *v,
            Expr::Name(n) => *env.get(n).ok_or_else(|| format!("there is no parameter `{n}`"))?,
            Expr::Neg(e) => -e.eval(env)?,
            Expr::Bin(op, a, b) => {
                let (a, b) = (a.eval(env)?, b.eval(env)?);
                match op {
                    Op::Add => a + b,
                    Op::Sub => a - b,
                    Op::Mul => a * b,
                    Op::Div if b == 0.0 => return Err("division by zero".into()),
                    Op::Div => a / b,
                    Op::Pow => a.powf(b),
                }
            }
            Expr::Call(f, args) => {
                let v: Vec<f64> = args.iter().map(|a| a.eval(env)).collect::<Result<_, _>>()?;
                let one = |g: fn(f64) -> f64| match v.as_slice() {
                    [x] => Ok(g(*x)),
                    _ => Err(format!("`{f}` takes one value")),
                };
                match f.as_str() {
                    // Trigonometry in degrees, like the angles shown.
                    "sin" => one(|x| x.to_radians().sin())?,
                    "cos" => one(|x| x.to_radians().cos())?,
                    "tan" => one(|x| x.to_radians().tan())?,
                    "asin" => one(|x| x.asin().to_degrees())?,
                    "acos" => one(|x| x.acos().to_degrees())?,
                    "atan" => one(|x| x.atan().to_degrees())?,
                    "sqrt" => one(f64::sqrt)?,
                    "abs" => one(f64::abs)?,
                    "round" => one(f64::round)?,
                    "floor" => one(f64::floor)?,
                    "ceil" => one(f64::ceil)?,
                    "ln" => one(f64::ln)?,
                    "log" => one(f64::log10)?,
                    "exp" => one(f64::exp)?,
                    "min" | "max" if v.is_empty() => return Err(format!("`{f}` needs at least one value")),
                    "min" => v.iter().copied().fold(f64::INFINITY, f64::min),
                    "max" => v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                    _ => return Err(format!("there is no function `{f}`")),
                }
            }
        };
        if v.is_finite() { Ok(v) } else { Err("the result is not a finite number".into()) }
    }
}

/// True if `s` is a valid parameter name: a letter or `_`, then letters, digits or `_`, and
/// not a unit, constant or function name.
pub fn valid_name(s: &str) -> bool {
    let mut cs = s.chars();
    cs.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && cs.all(|c| c.is_alphanumeric() || c == '_')
        && s.len() <= 64
        && unit_factor(s).is_none()
        && !matches!(
            s,
            "PI" | "pi"
                | "E"
                | "sin"
                | "cos"
                | "tan"
                | "asin"
                | "acos"
                | "atan"
                | "sqrt"
                | "abs"
                | "round"
                | "floor"
                | "ceil"
                | "ln"
                | "log"
                | "exp"
                | "min"
                | "max"
        )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn ev(s: &str) -> Result<f64, String> {
        let env: BTreeMap<String, f64> = [("d0".to_string(), 20.0), ("width".to_string(), 50.0)].into();
        parse(s)?.eval(&env)
    }

    #[test]
    fn arithmetic_units_names_and_functions() {
        assert_eq!(ev("1 + 2 * 3").unwrap(), 7.0);
        assert_eq!(ev("(1 + 2) * 3").unwrap(), 9.0);
        assert_eq!(ev("-2 ^ 2").unwrap(), -4.0, "power binds tighter than minus");
        assert_eq!(ev("2 ^ 3 ^ 2").unwrap(), 512.0, "right-associative");
        assert_eq!(ev("10 mm + 1 cm").unwrap(), 20.0);
        assert_eq!(ev("1 in").unwrap(), 25.4);
        assert_eq!(ev("width / 2 - d0").unwrap(), 5.0);
        assert!((ev("sin(30)").unwrap() - 0.5).abs() < 1e-12, "degrees");
        assert!((ev("PI rad").unwrap() - 180.0).abs() < 1e-12);
        assert_eq!(ev("max(d0, width, 3)").unwrap(), 50.0);
        assert_eq!(ev("1.5e2").unwrap(), 150.0);
        assert_eq!(ev(".5").unwrap(), 0.5);
        let mut names = Vec::new();
        parse("width / 2 + d0 * width").unwrap().names(&mut names);
        assert_eq!(names, vec!["width".to_string(), "d0".to_string()]);
    }

    #[test]
    fn bad_equations_say_why() {
        for (s, why) in [
            ("", "empty"),
            ("1 +", "ends too early"),
            ("(1 + 2", "not closed"),
            ("2 3", "unexpected"),
            ("depth", "no parameter `depth`"),
            ("1 / 0", "division by zero"),
            ("frob(2)", "no function"),
            ("sqrt(1, 2)", "one value"),
            ("2 $ 3", "unexpected `$`"),
            ("sqrt(-1)", "not a finite"),
        ] {
            let e = ev(s).unwrap_err();
            assert!(e.contains(why), "{s}: {e}");
        }
        assert!(parse(&"(".repeat(200)).is_err());
        assert!(parse(&"1+".repeat(2000)).is_err());
        assert!(valid_name("width_2") && valid_name("d12") && !valid_name("2x") && !valid_name("mm") && !valid_name("sin") && !valid_name("a b"));
    }
}

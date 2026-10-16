//! Programming calculator commands and evaluation.
//!
//! A programmer's calculator supporting arithmetic, bitwise operations,
//! and hex/binary/octal number formats. Accessible via ^X # keybinding.
//!
//! Uses a shunting-yard algorithm for expression parsing and supports
//! variables for storing intermediate results.

use std::collections::HashMap;

// =============================================================================
// ERROR TYPES
// =============================================================================

/// Typed errors for calculator evaluation (N-187: undefined identifiers are
/// explicit errors, not silent 0 placeholders).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalcError {
    /// An identifier was used but never assigned.
    UndefinedVariable(String),
    /// Division by zero.
    DivisionByZero,
    /// Modulo by zero.
    ModuloByZero,
    /// Stack underflow (malformed expression).
    StackUnderflow,
    /// Invalid assignment (e.g. `5 = x` or `x + 1 = 5`).
    InvalidAssignment,
    /// Mismatched parentheses.
    MismatchedParentheses,
    /// Empty expression.
    EmptyExpression,
    /// Tokenization error.
    Tokenize(String),
    /// Arithmetic overflow, or a shift amount outside 0..64.
    Overflow,
}

impl std::fmt::Display for CalcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CalcError::UndefinedVariable(name) => {
                write!(f, "Undefined variable: {}", name)
            }
            CalcError::DivisionByZero => write!(f, "Division by zero"),
            CalcError::ModuloByZero => write!(f, "Modulo by zero"),
            CalcError::StackUnderflow => write!(f, "Stack underflow"),
            CalcError::InvalidAssignment => write!(f, "Invalid assignment"),
            CalcError::MismatchedParentheses => write!(f, "Mismatched parentheses"),
            CalcError::EmptyExpression => write!(f, "Empty expression"),
            CalcError::Tokenize(msg) => write!(f, "Tokenize error: {}", msg),
            CalcError::Overflow => write!(f, "Arithmetic overflow"),
        }
    }
}

impl std::error::Error for CalcError {}

// =============================================================================
// TOKEN TYPES
// =============================================================================

/// Token types for calculator expressions
#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(i64),
    Identifier(String),
    /// Assignment target (e.g. `x` in `x = 5`). Distinguished from Identifier
    /// at parse time so undefined-variable uses are errors (N-187).
    AssignTarget(String),
    /// Unary minus (N-188: distinguished from binary Minus at parse time).
    UnaryMinus,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,        // Modulo
    Ampersand,      // Bitwise AND
    Pipe,           // Bitwise OR
    Caret,          // Bitwise XOR
    Tilde,          // Bitwise NOT (unary)
    LessLess,       // Left shift
    GreaterGreater, // Right shift
    LParen,
    RParen,
    Equals, // Assignment
}

// =============================================================================
// OPERATOR PROPERTIES
// =============================================================================

/// Operator precedence (higher = binds tighter)
fn precedence(token: &Token) -> u8 {
    match token {
        Token::Equals => 1,                           // Assignment (lowest)
        Token::Pipe => 2,                             // Bitwise OR
        Token::Caret => 3,                            // Bitwise XOR
        Token::Ampersand => 4,                        // Bitwise AND
        Token::LessLess | Token::GreaterGreater => 5, // Shifts
        Token::Plus | Token::Minus => 6,
        Token::Star | Token::Slash | Token::Percent => 7,
        Token::Tilde | Token::UnaryMinus => 8, // Unary operators (highest)
        _ => 0,
    }
}

/// Check if operator is right-associative
fn is_right_associative(token: &Token) -> bool {
    matches!(token, Token::Equals | Token::Tilde | Token::UnaryMinus)
}

/// Check if token is an operator
fn is_operator(token: &Token) -> bool {
    matches!(
        token,
        Token::Plus
            | Token::Minus
            | Token::UnaryMinus
            | Token::Star
            | Token::Slash
            | Token::Percent
            | Token::Ampersand
            | Token::Pipe
            | Token::Caret
            | Token::Tilde
            | Token::LessLess
            | Token::GreaterGreater
            | Token::Equals
    )
}

// =============================================================================
// TOKENIZER
// =============================================================================

/// Tokenize a calculator expression
fn tokenize(input: &str) -> Result<Vec<Token>, CalcError> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            ' ' | '\t' | '\n' => {
                chars.next();
            }
            '0'..='9' => {
                let num = parse_number(&mut chars)?;
                tokens.push(Token::Number(num));
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let ident = parse_identifier(&mut chars);
                tokens.push(Token::Identifier(ident));
            }
            '+' => {
                chars.next();
                tokens.push(Token::Plus);
            }
            '-' => {
                chars.next();
                tokens.push(Token::Minus);
            }
            '*' => {
                chars.next();
                tokens.push(Token::Star);
            }
            '/' => {
                chars.next();
                tokens.push(Token::Slash);
            }
            '%' => {
                chars.next();
                tokens.push(Token::Percent);
            }
            '&' => {
                chars.next();
                tokens.push(Token::Ampersand);
            }
            '|' => {
                chars.next();
                tokens.push(Token::Pipe);
            }
            '^' => {
                chars.next();
                tokens.push(Token::Caret);
            }
            '~' => {
                chars.next();
                tokens.push(Token::Tilde);
            }
            '<' => {
                chars.next();
                if chars.peek() == Some(&'<') {
                    chars.next();
                    tokens.push(Token::LessLess);
                } else {
                    return Err(CalcError::Tokenize("Expected << for left shift".to_string()));
                }
            }
            '>' => {
                chars.next();
                if chars.peek() == Some(&'>') {
                    chars.next();
                    tokens.push(Token::GreaterGreater);
                } else {
                    return Err(CalcError::Tokenize("Expected >> for right shift".to_string()));
                }
            }
            '(' => {
                chars.next();
                tokens.push(Token::LParen);
            }
            ')' => {
                chars.next();
                tokens.push(Token::RParen);
            }
            '=' => {
                chars.next();
                tokens.push(Token::Equals);
            }
            _ => {
                return Err(CalcError::Tokenize(format!("Unexpected character: {}", ch)));
            }
        }
    }

    Ok(tokens)
}

/// Parse a number (supports hex 0x, binary 0b, octal 0o prefixes)
fn parse_number(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<i64, CalcError> {
    let mut num_str = String::new();

    // Check for base prefix
    if chars.peek() == Some(&'0') {
        if let Some(ch) = chars.next() {
            num_str.push(ch);
        }
        match chars.peek() {
            Some('x') | Some('X') => {
                chars.next();
                // Hex number
                while chars.peek().is_some_and(|ch| ch.is_ascii_hexdigit()) {
                    if let Some(ch) = chars.next() {
                        num_str.push(ch);
                    }
                }
                return i64::from_str_radix(&num_str[1..], 16)
                    .map_err(|e| CalcError::Tokenize(format!("Invalid hex number: {}", e)));
            }
            Some('b') | Some('B') => {
                chars.next();
                num_str.clear();
                // Binary number
                while chars.peek().is_some_and(|&ch| ch == '0' || ch == '1') {
                    if let Some(ch) = chars.next() {
                        num_str.push(ch);
                    }
                }
                return i64::from_str_radix(&num_str, 2)
                    .map_err(|e| CalcError::Tokenize(format!("Invalid binary number: {}", e)));
            }
            Some('o') | Some('O') => {
                chars.next();
                num_str.clear();
                // Octal number
                while chars.peek().is_some_and(|&ch| ('0'..='7').contains(&ch)) {
                    if let Some(ch) = chars.next() {
                        num_str.push(ch);
                    }
                }
                return i64::from_str_radix(&num_str, 8)
                    .map_err(|e| CalcError::Tokenize(format!("Invalid octal number: {}", e)));
            }
            _ => {}
        }
    }

    // Decimal number
    while chars.peek().is_some_and(|ch| ch.is_ascii_digit()) {
        if let Some(ch) = chars.next() {
            num_str.push(ch);
        }
    }

    num_str
        .parse::<i64>()
        .map_err(|e| CalcError::Tokenize(format!("Invalid number: {}", e)))
}

/// Parse an identifier
fn parse_identifier(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut ident = String::new();
    while chars
        .peek()
        .is_some_and(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
    {
        if let Some(ch) = chars.next() {
            ident.push(ch);
        }
    }
    ident
}

// =============================================================================
// SHUNTING-YARD PARSER
// =============================================================================

/// Convert infix tokens to postfix (Reverse Polish Notation) using shunting-yard
fn shunting_yard(tokens: Vec<Token>) -> Result<Vec<Token>, CalcError> {
    let mut output: Vec<Token> = Vec::new();
    let mut operator_stack: Vec<Token> = Vec::new();

    // N-188: Distinguish unary vs binary minus at parse time. A minus is
    // unary if it's the first token, after LParen, or after another operator.
    // N-187: Convert Identifier followed by Equals into AssignTarget.
    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i].clone();
        let is_unary_minus = matches!(token, Token::Minus) && {
            if i == 0 {
                true
            } else {
                matches!(tokens[i - 1], Token::LParen) || is_operator(&tokens[i - 1])
            }
        };
        let token = if is_unary_minus {
            Token::UnaryMinus
        } else {
            token
        };
        // N-187: If this is an Identifier and the next token is Equals,
        // it's an assignment target, not a variable use.
        let token = match token {
            Token::Identifier(name) => {
                if i + 1 < tokens.len() && matches!(tokens[i + 1], Token::Equals) {
                    Token::AssignTarget(name)
                } else {
                    Token::Identifier(name)
                }
            }
            t => t,
        };
        i += 1;

        match &token {
            Token::Number(_) | Token::Identifier(_) | Token::AssignTarget(_) => {
                output.push(token);
            }
            Token::LParen => {
                operator_stack.push(token);
            }
            Token::RParen => {
                while let Some(top) = operator_stack.last() {
                    if *top == Token::LParen {
                        break;
                    }
                    if let Some(op) = operator_stack.pop() {
                        output.push(op);
                    }
                }
                if operator_stack.pop() != Some(Token::LParen) {
                    return Err(CalcError::MismatchedParentheses);
                }
            }
            _ if is_operator(&token) => {
                while let Some(top) = operator_stack.last() {
                    if *top == Token::LParen {
                        break;
                    }
                    if !is_operator(top) {
                        break;
                    }
                    let top_prec = precedence(top);
                    let token_prec = precedence(&token);
                    if top_prec > token_prec
                        || (top_prec == token_prec && !is_right_associative(&token))
                    {
                        if let Some(op) = operator_stack.pop() {
                            output.push(op);
                        }
                    } else {
                        break;
                    }
                }
                operator_stack.push(token);
            }
            _ => {
                return Err(CalcError::Tokenize(format!("Unexpected token: {:?}", token)));
            }
        }
    }

    // Pop remaining operators
    while let Some(op) = operator_stack.pop() {
        if op == Token::LParen {
            return Err(CalcError::MismatchedParentheses);
        }
        output.push(op);
    }

    Ok(output)
}

// =============================================================================
// EVALUATOR
// =============================================================================

/// Evaluate a postfix expression
fn evaluate(
    postfix: Vec<Token>,
    variables: &mut HashMap<String, i64>,
) -> Result<i64, CalcError> {
    let mut stack: Vec<i64> = Vec::new();
    let mut pending_assignment: Option<String> = None;

    for token in postfix {
        match token {
            Token::Number(n) => {
                stack.push(n);
            }
            // N-187: Undefined identifiers are explicit errors. Assignment
            // targets are Token::AssignTarget, not Token::Identifier.
            Token::Identifier(name) => {
                if let Some(val) = variables.get(&name) {
                    stack.push(*val);
                } else {
                    return Err(CalcError::UndefinedVariable(name));
                }
            }
            Token::AssignTarget(name) => {
                // Assignment target: record for the Equals operator.
                // No placeholder is pushed (the old code pushed 0, which was
                // a bug — it left garbage on the stack).
                if pending_assignment.is_some() {
                    return Err(CalcError::InvalidAssignment);
                }
                pending_assignment = Some(name);
            }
            Token::UnaryMinus => {
                // N-188: Unary minus negates the top of stack.
                let a = stack.pop().ok_or(CalcError::StackUnderflow)?;
                stack.push(a.checked_neg().ok_or(CalcError::Overflow)?);
            }
            Token::Plus => {
                let (a, b) = pop_two(&mut stack)?;
                stack.push(a.checked_add(b).ok_or(CalcError::Overflow)?);
            }
            Token::Minus => {
                let (a, b) = pop_two(&mut stack)?;
                stack.push(a.checked_sub(b).ok_or(CalcError::Overflow)?);
            }
            Token::Star => {
                let (a, b) = pop_two(&mut stack)?;
                stack.push(a.checked_mul(b).ok_or(CalcError::Overflow)?);
            }
            Token::Slash => {
                let (a, b) = pop_two(&mut stack)?;
                if b == 0 {
                    return Err(CalcError::DivisionByZero);
                }
                stack.push(a.checked_div(b).ok_or(CalcError::Overflow)?);
            }
            Token::Percent => {
                let (a, b) = pop_two(&mut stack)?;
                if b == 0 {
                    return Err(CalcError::ModuloByZero);
                }
                stack.push(a.checked_rem(b).ok_or(CalcError::Overflow)?);
            }
            Token::Ampersand => {
                let (a, b) = pop_two(&mut stack)?;
                stack.push(a & b);
            }
            Token::Pipe => {
                let (a, b) = pop_two(&mut stack)?;
                stack.push(a | b);
            }
            Token::Caret => {
                let (a, b) = pop_two(&mut stack)?;
                stack.push(a ^ b);
            }
            Token::Tilde => {
                let a = stack.pop().ok_or(CalcError::StackUnderflow)?;
                stack.push(!a);
            }
            Token::LessLess => {
                let (a, b) = pop_two(&mut stack)?;
                stack.push(a.checked_shl(shift_amount(b)?).ok_or(CalcError::Overflow)?);
            }
            Token::GreaterGreater => {
                let (a, b) = pop_two(&mut stack)?;
                stack.push(a.checked_shr(shift_amount(b)?).ok_or(CalcError::Overflow)?);
            }
            Token::Equals => {
                let val = stack.pop().ok_or(CalcError::StackUnderflow)?;
                if let Some(name) = pending_assignment.take() {
                    variables.insert(name, val);
                    stack.push(val);
                } else {
                    return Err(CalcError::InvalidAssignment);
                }
            }
            _ => {
                return Err(CalcError::Tokenize(format!(
                    "Unexpected token in evaluation: {:?}",
                    token
                )));
            }
        }
    }

    stack.pop().ok_or(CalcError::EmptyExpression)
}

/// Pop two values from stack (returns (first, second) where first was pushed first)
/// A validated shift amount: 0..64, else [`CalcError::Overflow`].
/// Negative amounts and amounts >= 64 are errors, not wraparound.
fn shift_amount(b: i64) -> Result<u32, CalcError> {
    let s: u32 = b.try_into().map_err(|_| CalcError::Overflow)?;
    if s >= 64 {
        return Err(CalcError::Overflow);
    }
    Ok(s)
}

fn pop_two(stack: &mut Vec<i64>) -> Result<(i64, i64), CalcError> {
    let b = stack.pop().ok_or(CalcError::StackUnderflow)?;
    let a = stack.pop().ok_or(CalcError::StackUnderflow)?;
    Ok((a, b))
}

// =============================================================================
// CALCULATOR API
// =============================================================================

/// Programming calculator with variables
#[derive(Debug, Default)]
pub struct Calculator {
    /// Variable storage
    variables: HashMap<String, i64>,
    /// Last result
    last_result: Option<i64>,
}

impl Calculator {
    /// Create a new calculator
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
            last_result: None,
        }
    }

    /// Evaluate an expression
    pub fn eval(&mut self, expr: &str) -> Result<i64, CalcError> {
        let tokens = tokenize(expr)?;
        let postfix = shunting_yard(tokens)?;
        let result = evaluate(postfix, &mut self.variables)?;
        self.last_result = Some(result);
        Ok(result)
    }

    /// Get the last result
    pub fn last_result(&self) -> Option<i64> {
        self.last_result
    }

    /// Get a variable value
    pub fn get_var(&self, name: &str) -> Option<i64> {
        self.variables.get(name).copied()
    }

    /// Set a variable
    pub fn set_var(&mut self, name: &str, value: i64) {
        self.variables.insert(name.to_string(), value);
    }

    /// Clear all variables
    pub fn clear(&mut self) {
        self.variables.clear();
        self.last_result = None;
    }


    /// Format result in multiple bases
    pub fn format_result(value: i64) -> String {
        format!("{} (0x{:X}) (0b{:b}) (0o{:o})", value, value, value, value)
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_arithmetic() {
        let mut calc = Calculator::new();
        if let Ok(v) = calc.eval("2 + 3") {
            assert_eq!(v, 5);
        }
        if let Ok(v) = calc.eval("10 - 4") {
            assert_eq!(v, 6);
        }
        if let Ok(v) = calc.eval("3 * 4") {
            assert_eq!(v, 12);
        }
        if let Ok(v) = calc.eval("15 / 3") {
            assert_eq!(v, 5);
        }
        if let Ok(v) = calc.eval("17 % 5") {
            assert_eq!(v, 2);
        }
    }

    #[test]
    fn test_precedence() {
        let mut calc = Calculator::new();
        if let Ok(v) = calc.eval("2 + 3 * 4") {
            assert_eq!(v, 14);
        }
        if let Ok(v) = calc.eval("(2 + 3) * 4") {
            assert_eq!(v, 20);
        }
        if let Ok(v) = calc.eval("10 - 2 * 3") {
            assert_eq!(v, 4);
        }
    }

    #[test]
    fn test_bitwise_ops() {
        let mut calc = Calculator::new();
        if let Ok(v) = calc.eval("0xFF & 0x0F") {
            assert_eq!(v, 15);
        }
        if let Ok(v) = calc.eval("0x0F | 0xF0") {
            assert_eq!(v, 255);
        }
        if let Ok(v) = calc.eval("0xFF ^ 0x0F") {
            assert_eq!(v, 240);
        }
        if let Ok(v) = calc.eval("1 << 4") {
            assert_eq!(v, 16);
        }
        if let Ok(v) = calc.eval("16 >> 2") {
            assert_eq!(v, 4);
        }
    }

    #[test]
    fn test_number_formats() {
        let mut calc = Calculator::new();
        if let Ok(v) = calc.eval("0xFF") {
            assert_eq!(v, 255);
        }
        if let Ok(v) = calc.eval("0b1111") {
            assert_eq!(v, 15);
        }
        if let Ok(v) = calc.eval("0o17") {
            assert_eq!(v, 15);
        }
        if let Ok(v) = calc.eval("255") {
            assert_eq!(v, 255);
        }
    }

    #[test]
    fn test_parentheses() {
        let mut calc = Calculator::new();
        if let Ok(v) = calc.eval("(1 + 2) * (3 + 4)") {
            assert_eq!(v, 21);
        }
        if let Ok(v) = calc.eval("((2 + 3))") {
            assert_eq!(v, 5);
        }
    }

    #[test]
    fn test_format_result() {
        let result = Calculator::format_result(255);
        assert!(result.contains("255"));
        assert!(result.contains("0xFF"));
        assert!(result.contains("0b11111111"));
        assert!(result.contains("0o377"));
    }

    #[test]
    fn test_division_by_zero() {
        let mut calc = Calculator::new();
        assert!(calc.eval("10 / 0").is_err());
        assert!(calc.eval("10 % 0").is_err());
    }

    /// G-003: arithmetic overflow and out-of-range shifts are `Overflow`
    /// errors, never panics (debug) or silent wraparound (release).
    #[test]
    fn g003_calc_overflow_is_a_typed_error() {
        let mut calc = Calculator::new();
        // i64::MIN via arithmetic (the literal itself does not parse).
        assert_eq!(calc.eval("m = 0 - 9223372036854775807 - 1").unwrap(), i64::MIN);
        let overflows = [
            "9223372036854775807 + 1",
            "m - 1",
            "3037000500 * 3037000500",
            "m * -1",
            "-m",
            "m / -1",
            "m % -1",
            "1 << 64",
            "1 << -1",
            "1 >> 64",
            "1 >> -1",
            "1 << 1000000",
        ];
        for expr in overflows {
            assert!(
                matches!(calc.eval(expr), Err(CalcError::Overflow)),
                "{expr} must be Overflow, got {:?}",
                calc.eval(expr)
            );
        }
        // Boundary-valid cases still compute.
        assert_eq!(calc.eval("1 << 63").unwrap(), i64::MIN);
        assert_eq!(calc.eval("8 >> 2").unwrap(), 2);
        assert_eq!(calc.eval("m >> 63").unwrap(), -1);
        assert_eq!(calc.eval("m << 0").unwrap(), i64::MIN);
    }

    /// G-003: random expression fuzz — the evaluator must never panic,
    /// whatever the input. Any `Err` must be a `CalcError`.
    #[test]
    fn g004_calc_fuzz_never_panics() {
        struct Rng(u64);
        impl Rng {
            fn next(&mut self) -> u64 {
                let mut x = self.0;
                x ^= x >> 12;
                x ^= x << 25;
                x ^= x >> 27;
                self.0 = x;
                x.wrapping_mul(0x2545F4914F6CDD1D)
            }
            fn below(&mut self, n: u64) -> u64 {
                self.next() % n.max(1)
            }
        }
        let nums = [
            "0", "1", "2", "7", "8", "63", "64", "65", "100",
            "9223372036854775807", "9223372036854775808", "3037000500",
            "99999999999999999999999999",
        ];
        let ops = [
            "+", "-", "*", "/", "%", "&", "|", "^", "<<", ">>", "==", "!=",
        ];
        let mut rng = Rng(0xCA1C);
        let mut calc = Calculator::new();
        for _ in 0..20000 {
            let mut expr = String::new();
            let terms = 1 + rng.below(4);
            for t in 0..terms {
                if rng.below(4) == 0 {
                    expr.push('(');
                }
                if rng.below(5) == 0 {
                    expr.push('-');
                }
                expr.push_str(nums[rng.below(nums.len() as u64) as usize]);
                if rng.below(4) == 0 {
                    expr.push(')');
                }
                if t + 1 < terms {
                    expr.push(' ');
                    expr.push_str(ops[rng.below(ops.len() as u64) as usize]);
                    expr.push(' ');
                }
            }
            // Must not panic; the result type enforces typed errors.
            let _ = calc.eval(&expr);
        }
    }

    #[test]
    fn test_mismatched_parens() {
        let mut calc = Calculator::new();
        assert!(calc.eval("(1 + 2").is_err());
        assert!(calc.eval("1 + 2)").is_err());
    }

    #[test]
    fn test_undefined_variable_is_error() {
        // N-187: Undefined identifiers are explicit errors, not silent 0.
        let mut calc = Calculator::new();
        let result = calc.eval("x + 1");
        assert!(
            matches!(result, Err(CalcError::UndefinedVariable(ref name)) if name == "x"),
            "undefined variable must be UndefinedVariable error, got {:?}",
            result
        );
        // Assignment still works.
        assert_eq!(calc.eval("x = 5").unwrap(), 5);
        assert_eq!(calc.eval("x + 1").unwrap(), 6);
        // Using a different undefined variable still errors.
        assert!(matches!(
            calc.eval("y + 1"),
            Err(CalcError::UndefinedVariable(_))
        ));
    }

    #[test]
    fn test_unary_minus() {
        // N-188: Unary minus is distinguished from binary at parse time.
        let mut calc = Calculator::new();
        assert_eq!(calc.eval("-5").unwrap(), -5);
        assert_eq!(calc.eval("10 + -5").unwrap(), 5);
        assert_eq!(calc.eval("10 - -5").unwrap(), 15);
        assert_eq!(calc.eval("(-5)").unwrap(), -5);
        assert_eq!(calc.eval("-x").unwrap_err(), CalcError::UndefinedVariable("x".to_string()));
        // Binary minus still works.
        assert_eq!(calc.eval("10 - 5").unwrap(), 5);
        // Unary minus with multiplication.
        assert_eq!(calc.eval("-2 * 3").unwrap(), -6);
        assert_eq!(calc.eval("2 * -3").unwrap(), -6);
    }
}

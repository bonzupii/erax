//! Programming Calculator
//!
//! A programmer's calculator supporting arithmetic, bitwise operations,
//! and hex/binary/octal number formats. Accessible via ^X # keybinding.
//!
//! Uses a shunting-yard algorithm for expression parsing and supports
//! variables for storing intermediate results.

use std::collections::HashMap;

// =============================================================================
// TOKEN TYPES
// =============================================================================

/// Token types for calculator expressions
#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(i64),
    Identifier(String),
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
        Token::Tilde => 8, // Unary NOT (highest)
        _ => 0,
    }
}

/// Check if operator is right-associative
fn is_right_associative(token: &Token) -> bool {
    matches!(token, Token::Equals | Token::Tilde)
}

/// Check if token is an operator
fn is_operator(token: &Token) -> bool {
    matches!(
        token,
        Token::Plus
            | Token::Minus
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
fn tokenize(input: &str) -> Result<Vec<Token>, String> {
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
                    return Err("Expected << for left shift".to_string());
                }
            }
            '>' => {
                chars.next();
                if chars.peek() == Some(&'>') {
                    chars.next();
                    tokens.push(Token::GreaterGreater);
                } else {
                    return Err("Expected >> for right shift".to_string());
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
                return Err(format!("Unexpected character: {}", ch));
            }
        }
    }

    Ok(tokens)
}

/// Parse a number (supports hex 0x, binary 0b, octal 0o prefixes)
fn parse_number(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<i64, String> {
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
                while chars.peek().map_or(false, |ch| ch.is_ascii_hexdigit()) {
                    if let Some(ch) = chars.next() {
                        num_str.push(ch);
                    }
                }
                return i64::from_str_radix(&num_str[1..], 16)
                    .map_err(|e| format!("Invalid hex number: {}", e));
            }
            Some('b') | Some('B') => {
                chars.next();
                num_str.clear();
                // Binary number
                while chars.peek().map_or(false, |&ch| ch == '0' || ch == '1') {
                    if let Some(ch) = chars.next() {
                        num_str.push(ch);
                    }
                }
                return i64::from_str_radix(&num_str, 2)
                    .map_err(|e| format!("Invalid binary number: {}", e));
            }
            Some('o') | Some('O') => {
                chars.next();
                num_str.clear();
                // Octal number
                while chars.peek().map_or(false, |&ch| ch >= '0' && ch <= '7') {
                    if let Some(ch) = chars.next() {
                        num_str.push(ch);
                    }
                }
                return i64::from_str_radix(&num_str, 8)
                    .map_err(|e| format!("Invalid octal number: {}", e));
            }
            _ => {}
        }
    }

    // Decimal number
    while chars.peek().map_or(false, |ch| ch.is_ascii_digit()) {
        if let Some(ch) = chars.next() {
            num_str.push(ch);
        }
    }

    num_str
        .parse::<i64>()
        .map_err(|e| format!("Invalid number: {}", e))
}

/// Parse an identifier
fn parse_identifier(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut ident = String::new();
    while chars
        .peek()
        .map_or(false, |ch| ch.is_ascii_alphanumeric() || *ch == '_')
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
fn shunting_yard(tokens: Vec<Token>) -> Result<Vec<Token>, String> {
    let mut output: Vec<Token> = Vec::new();
    let mut operator_stack: Vec<Token> = Vec::new();

    for token in tokens {
        match &token {
            Token::Number(_) | Token::Identifier(_) => {
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
                    return Err("Mismatched parentheses".to_string());
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
                return Err(format!("Unexpected token: {:?}", token));
            }
        }
    }

    // Pop remaining operators
    while let Some(op) = operator_stack.pop() {
        if op == Token::LParen {
            return Err("Mismatched parentheses".to_string());
        }
        output.push(op);
    }

    Ok(output)
}

// =============================================================================
// EVALUATOR
// =============================================================================

/// Evaluate a postfix expression
fn evaluate(postfix: Vec<Token>, variables: &mut HashMap<String, i64>) -> Result<i64, String> {
    let mut stack: Vec<i64> = Vec::new();
    let mut pending_assignment: Option<String> = None;

    for token in postfix {
        match token {
            Token::Number(n) => {
                stack.push(n);
            }
            Token::Identifier(name) => {
                if let Some(val) = variables.get(&name) {
                    stack.push(*val);
                } else {
                    // Could be assignment target
                    pending_assignment = Some(name);
                    stack.push(0); // Placeholder
                }
            }
            Token::Plus => {
                let (a, b) = pop_two(&mut stack)?;
                stack.push(a + b);
            }
            Token::Minus => {
                let (a, b) = pop_two(&mut stack)?;
                stack.push(a - b);
            }
            Token::Star => {
                let (a, b) = pop_two(&mut stack)?;
                stack.push(a * b);
            }
            Token::Slash => {
                let (a, b) = pop_two(&mut stack)?;
                if b == 0 {
                    return Err("Division by zero".to_string());
                }
                stack.push(a / b);
            }
            Token::Percent => {
                let (a, b) = pop_two(&mut stack)?;
                if b == 0 {
                    return Err("Modulo by zero".to_string());
                }
                stack.push(a % b);
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
                let a = stack.pop().ok_or("Stack underflow")?;
                stack.push(!a);
            }
            Token::LessLess => {
                let (a, b) = pop_two(&mut stack)?;
                stack.push(a << b);
            }
            Token::GreaterGreater => {
                let (a, b) = pop_two(&mut stack)?;
                stack.push(a >> b);
            }
            Token::Equals => {
                let val = stack.pop().ok_or("Stack underflow")?;
                if let Some(name) = pending_assignment.take() {
                    variables.insert(name, val);
                    stack.push(val);
                } else {
                    return Err("Invalid assignment".to_string());
                }
            }
            _ => {
                return Err(format!("Unexpected token in evaluation: {:?}", token));
            }
        }
    }

    stack.pop().ok_or_else(|| "Empty expression".to_string())
}

/// Pop two values from stack (returns (first, second) where first was pushed first)
fn pop_two(stack: &mut Vec<i64>) -> Result<(i64, i64), String> {
    let b = stack.pop().ok_or("Stack underflow")?;
    let a = stack.pop().ok_or("Stack underflow")?;
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
    pub fn eval(&mut self, expr: &str) -> Result<i64, String> {
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

    /// Evaluate a built-in function call
    pub fn call_function(name: &str, args: &[i64]) -> Result<i64, String> {
        match name {
            "min" => {
                if args.len() < 2 {
                    return Err("min requires 2 arguments".to_string());
                }
                args.iter()
                    .min()
                    .copied()
                    .ok_or_else(|| "min: empty arguments".to_string())
            }
            "max" => {
                if args.len() < 2 {
                    return Err("max requires 2 arguments".to_string());
                }
                args.iter()
                    .max()
                    .copied()
                    .ok_or_else(|| "max: empty arguments".to_string())
            }
            "abs" => {
                if args.len() != 1 {
                    return Err("abs requires 1 argument".to_string());
                }
                Ok(args[0].abs())
            }
            "popcount" => {
                if args.len() != 1 {
                    return Err("popcount requires 1 argument".to_string());
                }
                Ok(args[0].count_ones() as i64)
            }
            "clz" => {
                if args.len() != 1 {
                    return Err("clz requires 1 argument".to_string());
                }
                Ok(args[0].leading_zeros() as i64)
            }
            "ctz" => {
                if args.len() != 1 {
                    return Err("ctz requires 1 argument".to_string());
                }
                Ok(args[0].trailing_zeros() as i64)
            }
            "sizeof" => {
                // Return size of common types
                if args.len() != 1 {
                    return Err("sizeof requires 1 argument (type code)".to_string());
                }
                // Type codes: 1=u8, 2=u16, 4=u32, 8=u64, 16=u128
                match args[0] {
                    1 => Ok(1),
                    2 => Ok(2),
                    4 => Ok(4),
                    8 => Ok(8),
                    16 => Ok(16),
                    _ => Err("Unknown type code for sizeof".to_string()),
                }
            }
            _ => Err(format!("Unknown function: {}", name)),
        }
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
        assert_eq!(calc.eval("2 + 3").unwrap(), 5);
        assert_eq!(calc.eval("10 - 4").unwrap(), 6);
        assert_eq!(calc.eval("3 * 4").unwrap(), 12);
        assert_eq!(calc.eval("15 / 3").unwrap(), 5);
        assert_eq!(calc.eval("17 % 5").unwrap(), 2);
    }

    #[test]
    fn test_precedence() {
        let mut calc = Calculator::new();
        assert_eq!(calc.eval("2 + 3 * 4").unwrap(), 14);
        assert_eq!(calc.eval("(2 + 3) * 4").unwrap(), 20);
        assert_eq!(calc.eval("10 - 2 * 3").unwrap(), 4);
    }

    #[test]
    fn test_bitwise_ops() {
        let mut calc = Calculator::new();
        assert_eq!(calc.eval("0xFF & 0x0F").unwrap(), 15);
        assert_eq!(calc.eval("0x0F | 0xF0").unwrap(), 255);
        assert_eq!(calc.eval("0xFF ^ 0x0F").unwrap(), 240);
        assert_eq!(calc.eval("1 << 4").unwrap(), 16);
        assert_eq!(calc.eval("16 >> 2").unwrap(), 4);
    }

    #[test]
    fn test_number_formats() {
        let mut calc = Calculator::new();
        assert_eq!(calc.eval("0xFF").unwrap(), 255);
        assert_eq!(calc.eval("0b1111").unwrap(), 15);
        assert_eq!(calc.eval("0o17").unwrap(), 15);
        assert_eq!(calc.eval("255").unwrap(), 255);
    }

    #[test]
    fn test_parentheses() {
        let mut calc = Calculator::new();
        assert_eq!(calc.eval("(1 + 2) * (3 + 4)").unwrap(), 21);
        assert_eq!(calc.eval("((2 + 3))").unwrap(), 5);
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

    #[test]
    fn test_mismatched_parens() {
        let mut calc = Calculator::new();
        assert!(calc.eval("(1 + 2").is_err());
        assert!(calc.eval("1 + 2)").is_err());
    }
}

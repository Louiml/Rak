use crate::ast::*;
use crate::lexer::Token;
use crate::RakError;
use crate::Result;

/// Recursive descent parser for Rak.
pub fn parse(tokens: &[Token]) -> Result<Module> {
    let mut parser = Parser::new(tokens);
    parser.parse_module()
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&Token> {
        let tok = self.tokens.get(self.pos);
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, expected: Token) -> Result<&Token> {
        match self.peek() {
            Some(tok) if std::mem::discriminant(tok) == std::mem::discriminant(&expected) => {
                Ok(self.advance().unwrap())
            }
            Some(tok) => Err(RakError::Parser(format!(
                "Expected {:?}, found {:?}",
                expected, tok
            ))),
            None => Err(RakError::Parser(format!(
                "Expected {:?}, found end of file",
                expected
            ))),
        }
    }

    fn match_token(&mut self, expected: &Token) -> bool {
        match self.peek() {
            Some(tok) if std::mem::discriminant(tok) == std::mem::discriminant(expected) => {
                self.pos += 1;
                true
            }
            _ => false,
        }
    }

    /// Consume a semicolon if present. Always succeeds - semicolons are optional.
    fn semi(&mut self) -> Result<()> {
        let _ = self.match_token(&Token::Semi);
        Ok(())
    }

    fn parse_module(&mut self) -> Result<Module> {
        let mut imports = Vec::new();
        let mut items = Vec::new();

        while self.peek().is_some() {
            if self.match_token(&Token::Use) {
                imports.push(self.parse_import()?);
            } else {
                items.push(self.parse_stmt()?);
            }
        }

        Ok(Module { imports, items })
    }

    fn parse_import(&mut self) -> Result<Import> {
        let mut path = vec![];
        if let Some(Token::Ident(name)) = self.peek() {
            path.push(name.clone());
            self.advance();
        } else {
            return Err(RakError::Parser("Expected import path".to_string()));
        }

        while self.match_token(&Token::Dot) {
            if let Some(Token::Ident(name)) = self.peek() {
                path.push(name.clone());
                self.advance();
            } else {
                return Err(RakError::Parser("Expected identifier after '.'".to_string()));
            }
        }

        Ok(Import { path, alias: None })
    }

    fn parse_stmt(&mut self) -> Result<Stmt> {
        match self.peek() {
            Some(Token::Let) => self.parse_let(),
            Some(Token::If) => self.parse_if(),
            Some(Token::Loop) => self.parse_loop(),
            Some(Token::While) => self.parse_while(),
            Some(Token::For) => self.parse_for(),
            Some(Token::Scan) => self.parse_scan(),
            Some(Token::Fetch) => self.parse_fetch(),
            Some(Token::Dump) => self.parse_dump(),
            Some(Token::Trace) => self.parse_trace(),
            Some(Token::Return) => self.parse_return(),
            Some(Token::Break) => {
                self.advance();
                self.semi()?;
                Ok(Stmt::Break)
            }
            Some(Token::Continue) => {
                self.advance();
                self.semi()?;
                Ok(Stmt::Continue)
            }
            Some(Token::Fn) => self.parse_fn(),
            Some(Token::Struct) => self.parse_struct(),
            Some(Token::Enum) => self.parse_enum(),
            Some(Token::Mod) => self.parse_mod(),
            Some(Token::Impl) => self.parse_impl(),
            Some(Token::Match) => self.parse_match(),
            _ => {
                let expr = self.parse_expr()?;
                if self.match_token(&Token::Semi) {
                    Ok(Stmt::Expr(Box::new(expr)))
                } else {
                    Ok(Stmt::Expr(Box::new(expr)))
                }
            }
        }
    }

    fn parse_let(&mut self) -> Result<Stmt> {
        self.expect(Token::Let)?;
        let mutable = self.match_token(&Token::Mut);

        let name = match self.peek() {
            Some(Token::Ident(n)) => {
                let n = n.clone();
                self.advance();
                n
            }
            _ => return Err(RakError::Parser("Expected variable name after 'let'".to_string())),
        };

        let type_hint = if self.match_token(&Token::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };

        self.expect(Token::Eq)?;
        let value = self.parse_expr()?;
        self.semi()?;

        Ok(Stmt::Let {
            name,
            mutable,
            value: Box::new(value),
            type_hint,
        })
    }

    fn parse_if(&mut self) -> Result<Stmt> {
        self.expect(Token::If)?;
        let cond = self.parse_expr()?;
        let then_branch = self.parse_block()?;

        let else_branch = if self.match_token(&Token::Else) {
            Some(self.parse_block()?)
        } else {
            None
        };

        Ok(Stmt::If {
            cond: Box::new(cond),
            then_branch,
            else_branch,
        })
    }

    fn parse_loop(&mut self) -> Result<Stmt> {
        self.expect(Token::Loop)?;
        let body = self.parse_block()?;
        Ok(Stmt::Loop(body))
    }

    fn parse_while(&mut self) -> Result<Stmt> {
        self.expect(Token::While)?;
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::While {
            cond: Box::new(cond),
            body,
        })
    }

    fn parse_for(&mut self) -> Result<Stmt> {
        self.expect(Token::For)?;
        let name = match self.peek() {
            Some(Token::Ident(n)) => {
                let n = n.clone();
                self.advance();
                n
            }
            _ => return Err(RakError::Parser("Expected variable name after 'for'".to_string())),
        };
        self.expect(Token::In)?;
        let iterable = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::For {
            name,
            iterable: Box::new(iterable),
            body,
        })
    }

    fn parse_scan(&mut self) -> Result<Stmt> {
        self.expect(Token::Scan)?;
        let target = self.parse_expr()?;
        let options = if self.match_token(&Token::LBrace) {
            let opts = self.parse_options()?;
            self.expect(Token::RBrace)?;
            opts
        } else {
            vec![]
        };

        let body = if self.match_token(&Token::LBrace) {
            let stmts = self.parse_stmts_until_rbrace()?;
            Some(stmts)
        } else {
            None
        };

        Ok(Stmt::Scan {
            target: Box::new(target),
            options,
            body,
        })
    }

    fn parse_fetch(&mut self) -> Result<Stmt> {
        self.expect(Token::Fetch)?;
        let target = self.parse_expr()?;
        let options = if self.match_token(&Token::LBrace) {
            let opts = self.parse_options()?;
            self.expect(Token::RBrace)?;
            opts
        } else {
            vec![]
        };

        let body = if self.match_token(&Token::LBrace) {
            let stmts = self.parse_stmts_until_rbrace()?;
            Some(stmts)
        } else {
            None
        };

        Ok(Stmt::Fetch {
            target: Box::new(target),
            options,
            body,
        })
    }

    fn parse_dump(&mut self) -> Result<Stmt> {
        self.expect(Token::Dump)?;
        let value = self.parse_expr()?;
        let target = if self.match_token(&Token::Comma) {
            Some(Box::new(self.parse_expr()?))
        } else {
            None
        };
        self.semi()?;
        Ok(Stmt::Dump {
            value: Box::new(value),
            target,
        })
    }

    fn parse_trace(&mut self) -> Result<Stmt> {
        self.expect(Token::Trace)?;
        let value = self.parse_expr()?;
        self.semi()?;
        Ok(Stmt::Trace {
            value: Box::new(value),
        })
    }

    fn parse_return(&mut self) -> Result<Stmt> {
        self.expect(Token::Return)?;
        if self.match_token(&Token::Semi) {
            Ok(Stmt::Return(None))
        } else {
            let expr = self.parse_expr()?;
            self.semi()?;
            Ok(Stmt::Return(Some(Box::new(expr))))
        }
    }

    fn parse_fn(&mut self) -> Result<Stmt> {
        self.expect(Token::Fn)?;
        let name = match self.peek() {
            Some(Token::Ident(n)) => {
                let n = n.clone();
                self.advance();
                n
            }
            _ => return Err(RakError::Parser("Expected function name".to_string())),
        };

        self.expect(Token::LParen)?;
        let params = self.parse_params()?;
        self.expect(Token::RParen)?;

        let return_type = if self.match_token(&Token::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };

        let body = self.parse_block()?;

        // We don't have a direct function declaration in Expr.
        // Let me store it as a let binding with a function expression.
        Ok(Stmt::Let {
            name,
            mutable: false,
            value: Box::new(Expr::Function {
                params,
                return_type,
                body,
            }),
            type_hint: None,
        })
    }

    fn parse_struct(&mut self) -> Result<Stmt> {
        self.expect(Token::Struct)?;
        let name = match self.peek() {
            Some(Token::Ident(n)) => {
                let n = n.clone();
                self.advance();
                n
            }
            _ => return Err(RakError::Parser("Expected struct name".to_string())),
        };
        self.expect(Token::LBrace)?;
        let fields = self.parse_params()?;
        self.expect(Token::RBrace)?;
        Ok(Stmt::Struct { name, fields })
    }

    fn parse_enum(&mut self) -> Result<Stmt> {
        self.expect(Token::Enum)?;
        let name = match self.peek() {
            Some(Token::Ident(n)) => {
                let n = n.clone();
                self.advance();
                n
            }
            _ => return Err(RakError::Parser("Expected enum name".to_string())),
        };
        self.expect(Token::LBrace)?;
        let mut variants = vec![];
        while let Some(Token::Ident(v)) = self.peek() {
            variants.push(v.clone());
            self.advance();
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        self.expect(Token::RBrace)?;
        Ok(Stmt::Enum { name, variants })
    }

    fn parse_mod(&mut self) -> Result<Stmt> {
        self.expect(Token::Mod)?;
        let name = match self.peek() {
            Some(Token::Ident(n)) => {
                let n = n.clone();
                self.advance();
                n
            }
            _ => return Err(RakError::Parser("Expected module name".to_string())),
        };
        let items = self.parse_block()?;
        Ok(Stmt::Mod { name, items })
    }

    fn parse_impl(&mut self) -> Result<Stmt> {
        self.expect(Token::Impl)?;
        let target = match self.peek() {
            Some(Token::Ident(n)) => {
                let n = n.clone();
                self.advance();
                n
            }
            _ => return Err(RakError::Parser("Expected type name".to_string())),
        };
        self.expect(Token::LBrace)?;
        let mut methods = vec![];
        while self.peek() != Some(&Token::RBrace) && self.peek().is_some() {
            methods.push(self.parse_fn()?);
        }
        self.expect(Token::RBrace)?;
        Ok(Stmt::Impl { target, methods })
    }

    fn parse_match(&mut self) -> Result<Stmt> {
        self.expect(Token::Match)?;
        let value = self.parse_expr()?;
        self.expect(Token::LBrace)?;
        let mut arms = vec![];
        while self.peek() != Some(&Token::RBrace) && self.peek().is_some() {
            let pattern = self.parse_pattern()?;
            self.expect(Token::FatArrow)?;
            let body = if self.match_token(&Token::LBrace) {
                self.parse_stmts_until_rbrace()?
            } else {
                let expr = self.parse_expr()?;
                self.expect(Token::Comma)?;
                vec![Stmt::Expr(Box::new(expr))]
            };
            arms.push((pattern, body));
        }
        self.expect(Token::RBrace)?;
        Ok(Stmt::Match {
            value: Box::new(value),
            arms,
        })
    }

    fn parse_pattern(&mut self) -> Result<Pattern> {
        match self.peek() {
            Some(Token::Ident(n)) => {
                let n = n.clone();
                self.advance();
                if n == "_" {
                    Ok(Pattern::Wild)
                } else {
                    Ok(Pattern::Ident(n))
                }
            }
            Some(Token::Hex(h)) => {
                let h = *h;
                self.advance();
                Ok(Pattern::Hex(h))
            }
            Some(Token::Int(i)) => {
                let i = *i;
                self.advance();
                Ok(Pattern::Int(i))
            }
            Some(Token::String(s)) => {
                let s = s.clone();
                self.advance();
                Ok(Pattern::String(s))
            }
            Some(Token::True) => {
                self.advance();
                Ok(Pattern::Bool(true))
            }
            Some(Token::False) => {
                self.advance();
                Ok(Pattern::Bool(false))
            }
            Some(Token::Nil) => {
                self.advance();
                Ok(Pattern::Nil)
            }
            _ => Err(RakError::Parser("Expected pattern".to_string())),
        }
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>> {
        self.expect(Token::LBrace)?;
        self.parse_stmts_until_rbrace()
    }

    fn parse_stmts_until_rbrace(&mut self) -> Result<Vec<Stmt>> {
        let mut stmts = vec![];
        while self.peek() != Some(&Token::RBrace) && self.peek().is_some() {
            stmts.push(self.parse_stmt()?);
        }
        self.expect(Token::RBrace)?;
        Ok(stmts)
    }

    fn parse_options(&mut self) -> Result<Vec<(String, Expr)>> {
        let mut options = vec![];
        while self.peek() != Some(&Token::RBrace) && self.peek().is_some() {
            let key = match self.peek() {
                Some(Token::Ident(k)) => {
                    let k = k.clone();
                    self.advance();
                    k
                }
                _ => break,
            };
            self.expect(Token::Colon)?;
            let value = self.parse_expr()?;
            options.push((key, value));
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        Ok(options)
    }

    fn parse_params(&mut self) -> Result<Vec<Param>> {
        let mut params = vec![];
        while self.peek() != Some(&Token::RParen) && self.peek() != Some(&Token::RBrace) && self.peek().is_some() {
            let name = match self.peek() {
                Some(Token::Ident(n)) => {
                    let n = n.clone();
                    self.advance();
                    n
                }
                _ => break,
            };
            let type_hint = if self.match_token(&Token::Colon) {
                Some(self.parse_type()?)
            } else {
                None
            };
            params.push(Param { name, type_hint });
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        Ok(params)
    }

    fn parse_type(&mut self) -> Result<Type> {
        match self.peek() {
            Some(Token::Ident(name)) => {
                let name = name.clone();
                self.advance();
                match name.as_str() {
                    "hex8" => Ok(Type::Hex(8)),
                    "hex16" => Ok(Type::Hex(16)),
                    "hex32" => Ok(Type::Hex(32)),
                    "hex64" => Ok(Type::Hex(64)),
                    "int" => Ok(Type::Int),
                    "string" => Ok(Type::String),
                    "bytes" => Ok(Type::Bytes),
                    "bool" => Ok(Type::Bool),
                    "nil" => Ok(Type::Nil),
                    _ => Ok(Type::Custom(name)),
                }
            }
            _ => Err(RakError::Parser("Expected type".to_string())),
        }
    }

    // --- Expression parsing (pratt parser / recursive descent) ---

    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> Result<Expr> {
        let mut expr = self.parse_or()?;

        if self.match_token(&Token::Eq) {
            let value = self.parse_expr()?;
            // Only allow assignment to identifiers for now
            if let Expr::Ident(name) = expr {
                expr = Expr::Assign(name, Box::new(value));
            } else {
                return Err(RakError::Parser(
                    "Invalid assignment target".to_string(),
                ));
            }
        }

        Ok(expr)
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        while self.match_token(&Token::OrOr) {
            let right = self.parse_and()?;
            left = Expr::Binary(BinOp::Or, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut left = self.parse_equality()?;
        while self.match_token(&Token::AndAnd) {
            let right = self.parse_equality()?;
            left = Expr::Binary(BinOp::And, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<Expr> {
        let mut left = self.parse_comparison()?;
        loop {
            if self.match_token(&Token::EqEq) {
                let right = self.parse_comparison()?;
                left = Expr::Binary(BinOp::Eq, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::NotEq) {
                let right = self.parse_comparison()?;
                left = Expr::Binary(BinOp::NotEq, Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        let mut left = self.parse_bitwise()?;
        loop {
            if self.match_token(&Token::Lt) {
                let right = self.parse_bitwise()?;
                left = Expr::Binary(BinOp::Lt, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::Gt) {
                let right = self.parse_bitwise()?;
                left = Expr::Binary(BinOp::Gt, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::LtEq) {
                let right = self.parse_bitwise()?;
                left = Expr::Binary(BinOp::LtEq, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::GtEq) {
                let right = self.parse_bitwise()?;
                left = Expr::Binary(BinOp::GtEq, Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_bitwise(&mut self) -> Result<Expr> {
        let mut left = self.parse_shift()?;
        loop {
            if self.match_token(&Token::Ampersand) {
                let right = self.parse_shift()?;
                left = Expr::Binary(BinOp::BitAnd, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::Pipe) {
                let right = self.parse_shift()?;
                left = Expr::Binary(BinOp::BitOr, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::Caret) {
                let right = self.parse_shift()?;
                left = Expr::Binary(BinOp::BitXor, Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_shift(&mut self) -> Result<Expr> {
        let mut left = self.parse_term()?;
        loop {
            if self.match_token(&Token::Shl) {
                let right = self.parse_term()?;
                left = Expr::Binary(BinOp::Shl, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::Shr) {
                let right = self.parse_term()?;
                left = Expr::Binary(BinOp::Shr, Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_term(&mut self) -> Result<Expr> {
        let mut left = self.parse_factor()?;
        loop {
            if self.match_token(&Token::Plus) {
                let right = self.parse_factor()?;
                left = Expr::Binary(BinOp::Add, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::Minus) {
                let right = self.parse_factor()?;
                left = Expr::Binary(BinOp::Sub, Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_factor(&mut self) -> Result<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            if self.match_token(&Token::Star) {
                let right = self.parse_unary()?;
                left = Expr::Binary(BinOp::Mul, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::Slash) {
                let right = self.parse_unary()?;
                left = Expr::Binary(BinOp::Div, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::Percent) {
                let right = self.parse_unary()?;
                left = Expr::Binary(BinOp::Rem, Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if self.match_token(&Token::Minus) {
            let expr = self.parse_unary()?;
            Ok(Expr::Unary(UnOp::Minus, Box::new(expr)))
        } else if self.match_token(&Token::Bang) {
            let expr = self.parse_unary()?;
            Ok(Expr::Unary(UnOp::Not, Box::new(expr)))
        } else if self.match_token(&Token::Tilde) {
            let expr = self.parse_unary()?;
            Ok(Expr::Unary(UnOp::BitNot, Box::new(expr)))
        } else {
            self.parse_call()
        }
    }

    fn parse_call(&mut self) -> Result<Expr> {
        let mut expr = self.parse_primary()?;

        loop {
            if self.match_token(&Token::LParen) {
                let args = self.parse_args()?;
                self.expect(Token::RParen)?;
                expr = Expr::Call {
                    callee: Box::new(expr),
                    args,
                };
            } else if self.match_token(&Token::Dot) {
                let field = match self.peek() {
                    Some(Token::Ident(n)) => {
                        let n = n.clone();
                        self.advance();
                        n
                    }
                    _ => return Err(RakError::Parser("Expected field name".to_string())),
                };
                expr = Expr::FieldAccess(Box::new(expr), field);
            } else if self.match_token(&Token::LBracket) {
                let idx = self.parse_expr()?;
                self.expect(Token::RBracket)?;
                expr = Expr::Index(Box::new(expr), Box::new(idx));
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn parse_args(&mut self) -> Result<Vec<Expr>> {
        let mut args = vec![];
        while self.peek() != Some(&Token::RParen) && self.peek().is_some() {
            args.push(self.parse_expr()?);
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.peek() {
            Some(Token::Hex(h)) => {
                let h = *h;
                self.advance();
                Ok(Expr::Hex(h))
            }
            Some(Token::Int(i)) => {
                let i = *i;
                self.advance();
                Ok(Expr::Int(i))
            }
            Some(Token::String(s)) => {
                let s = s.clone();
                self.advance();
                Ok(Expr::String(s))
            }
            Some(Token::Bytes(b)) => {
                let b = b.clone();
                self.advance();
                Ok(Expr::Bytes(b))
            }
            Some(Token::True) => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            Some(Token::False) => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            Some(Token::Nil) => {
                self.advance();
                Ok(Expr::Nil)
            }
            Some(Token::Ident(name)) => {
                let name = name.clone();
                self.advance();
                Ok(Expr::Ident(name))
            }
            Some(Token::LParen) => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(Token::RParen)?;
                Ok(expr)
            }
            Some(Token::LBracket) => {
                self.advance();
                let mut elements = vec![];
                while self.peek() != Some(&Token::RBracket) && self.peek().is_some() {
                    elements.push(self.parse_expr()?);
                    if !self.match_token(&Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::RBracket)?;
                // Represent arrays as a call to a built-in array constructor for now
                Ok(Expr::Call {
                    callee: Box::new(Expr::Ident("array".to_string())),
                    args: elements,
                })
            }
            Some(Token::LBrace) => {
                self.advance();
                let mut pairs = vec![];
                while self.peek() != Some(&Token::RBrace) && self.peek().is_some() {
                    // Map keys can be identifiers (converted to strings) or string/expr literals
                    let key = match self.peek() {
                        Some(Token::Ident(name)) => {
                            let name = name.clone();
                            self.advance();
                            Expr::String(name)
                        }
                        _ => self.parse_expr()?,
                    };
                    self.expect(Token::Colon)?;
                    let value = self.parse_expr()?;
                    pairs.push(key);
                    pairs.push(value);
                    if !self.match_token(&Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::RBrace)?;
                Ok(Expr::Call {
                    callee: Box::new(Expr::Ident("map".to_string())),
                    args: pairs,
                })
            }
            Some(tok) => Err(RakError::Parser(format!(
                "Unexpected token in expression: {:?}",
                tok
            ))),
            None => Err(RakError::Parser("Unexpected end of input".to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    #[test]
    fn test_parse_simple_let() {
        let source = "let x = 0x1A;";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens).unwrap();
        assert_eq!(module.items.len(), 1);
        match &module.items[0] {
            Stmt::Let { name, value, .. } => {
                assert_eq!(name, "x");
                assert!(matches!(**value, Expr::Hex(0x1A)));
            }
            _ => panic!("Expected let statement"),
        }
    }

    #[test]
    fn test_parse_binop() {
        let source = "let sig = 0xDEAD & 0xFF00;";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens).unwrap();
        match &module.items[0] {
            Stmt::Let { value, .. } => {
                assert!(matches!(**value, Expr::Binary(BinOp::BitAnd, _, _)));
            }
            _ => panic!("Expected let statement"),
        }
    }

    #[test]
    fn test_parse_scan() {
        let source = "scan target { range: [0x0010, 0x0050] }";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens).unwrap();
        assert_eq!(module.items.len(), 1);
        assert!(matches!(&module.items[0], Stmt::Scan { .. }));
    }
}

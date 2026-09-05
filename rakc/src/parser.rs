use crate::ast::*;
use crate::lexer::{tokenize, Token};
use crate::RakError;
use crate::Result;

fn map_int_kind(k: crate::lexer::IntKind) -> IntKind {
    use crate::lexer::IntKind as L;
    match k {
        L::I8 => IntKind::I8,
        L::I16 => IntKind::I16,
        L::I32 => IntKind::I32,
        L::I64 => IntKind::I64,
        L::U8 => IntKind::U8,
        L::U16 => IntKind::U16,
        L::U32 => IntKind::U32,
        L::U64 => IntKind::U64,
    }
}

pub fn parse(tokens: &[Token]) -> Result<Module> {
    let mut parser = Parser::new(tokens);
    parser.parse_module()
}

pub fn parse_expr_str(source: &str) -> Result<Expr> {
    let tokens = tokenize(source)?;
    let mut p = Parser::new(&tokens);
    p.parse_expr()
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

    fn peek_n(&self, n: usize) -> Option<&Token> {
        self.tokens.get(self.pos + n)
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

    fn check(&self, expected: &Token) -> bool {
        matches!(self.peek(), Some(tok) if std::mem::discriminant(tok) == std::mem::discriminant(expected))
    }

    fn semi(&mut self) -> Result<()> {
        let _ = self.match_token(&Token::Semi);
        Ok(())
    }

    fn parse_module(&mut self) -> Result<Module> {
        let mut imports = Vec::new();
        let mut items = Vec::new();
        while self.peek().is_some() {
            if self.check(&Token::Use) {
                self.advance();
                imports.push(self.parse_import()?);
                self.semi()?;
            } else {
                items.push(self.parse_stmt()?);
            }
        }
        Ok(Module { imports, items })
    }

    fn parse_import(&mut self) -> Result<Import> {
        if let Some(Token::String(s)) = self.peek() {
            let s = s.clone();
            self.advance();
            let alias = if self.match_token(&Token::As) {
                if let Some(Token::Ident(a)) = self.peek() {
                    let a = a.clone();
                    self.advance();
                    Some(a)
                } else {
                    None
                }
            } else {
                None
            };
            return Ok(Import { path: vec![s], is_file: true, alias });
        }

        let mut path = vec![];
        if let Some(Token::Ident(name)) = self.peek() {
            path.push(name.clone());
            self.advance();
        } else {
            return Err(RakError::Parser("Expected import path".to_string()));
        }
        while self.match_token(&Token::Dot) || self.match_token(&Token::ColonColon) {
            if let Some(Token::Ident(name)) = self.peek() {
                path.push(name.clone());
                self.advance();
            } else {
                return Err(RakError::Parser("Expected identifier after path separator".to_string()));
            }
        }
        let alias = if self.match_token(&Token::As) {
            if let Some(Token::Ident(a)) = self.peek() {
                let a = a.clone();
                self.advance();
                Some(a)
            } else {
                None
            }
        } else {
            None
        };
        Ok(Import { path, is_file: false, alias })
    }

    fn parse_stmt(&mut self) -> Result<Stmt> {
        if self.check(&Token::Pub) {
            self.advance();
            let inner = self.parse_stmt()?;
            return Ok(Stmt::Export(Box::new(inner)));
        }
        match self.peek() {
            Some(Token::Let) => self.parse_let(),
            Some(Token::If) => Ok(Stmt::Expr(Box::new(self.parse_if_expr()?))),
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
            Some(Token::Match) => Ok(Stmt::Expr(Box::new(self.parse_match_expr()?))),
            Some(Token::Trait) => self.parse_trait(),
            Some(Token::Try) => self.parse_try(),
            Some(Token::Raise) => {
                self.advance();
                let e = self.parse_expr()?;
                self.semi()?;
                Ok(Stmt::Raise(Box::new(e)))
            }
            Some(Token::Type) => self.parse_type_alias(),
            Some(Token::Async) => {
                self.advance();
                let body = self.parse_block()?;
                Ok(Stmt::Async(body))
            }
            _ => {
                let expr = self.parse_expr()?;
                self.semi()?;
                Ok(Stmt::Expr(Box::new(expr)))
            }
        }
    }

    fn parse_let(&mut self) -> Result<Stmt> {
        self.expect(Token::Let)?;
        let mutable = self.match_token(&Token::Mut);

        if self.check(&Token::LParen) || self.check(&Token::LBracket) || self.check(&Token::LBrace) {
            let pattern = self.parse_pattern()?;
            let type_hint = if self.match_token(&Token::Colon) {
                Some(self.parse_type()?)
            } else {
                None
            };
            self.expect(Token::Eq)?;
            let value = self.parse_expr()?;
            self.semi()?;
            return Ok(Stmt::Let {
                name: "_".to_string(),
                pattern: Some(pattern),
                mutable,
                value: Box::new(value),
                type_hint,
            });
        }

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
            pattern: None,
            mutable,
            value: Box::new(value),
            type_hint,
        })
    }

    fn parse_if_expr(&mut self) -> Result<Expr> {
        self.expect(Token::If)?;
        let cond = self.parse_expr()?;
        let then_branch = self.parse_block()?;
        let else_branch = if self.match_token(&Token::Else) {
            if self.check(&Token::If) {
                let e = self.parse_if_expr()?;
                Some(vec![Stmt::Expr(Box::new(e))])
            } else {
                Some(self.parse_block()?)
            }
        } else {
            None
        };
        Ok(Expr::If {
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
            Some(self.parse_stmts_until_rbrace()?)
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
            Some(self.parse_stmts_until_rbrace()?)
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
        let (params, type_params) = self.parse_params_with_generics()?;
        let return_type = if self.match_token(&Token::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let body = self.parse_block()?;
        let _ = type_params;
        Ok(Stmt::Let {
            name,
            pattern: None,
            mutable: false,
            value: Box::new(Expr::Function {
                params,
                return_type,
                body,
                captures: vec![],
                is_async: false,
            }),
            type_hint: None,
        })
    }

    fn parse_struct(&mut self) -> Result<Stmt> {
        self.expect(Token::Struct)?;
        let name = self.expect_ident()?;
        let type_params = self.parse_type_params()?;
        self.expect(Token::LBrace)?;
        let fields = self.parse_params()?;
        self.expect(Token::RBrace)?;
        Ok(Stmt::Struct {
            name,
            type_params,
            fields,
        })
    }

    fn parse_enum(&mut self) -> Result<Stmt> {
        self.expect(Token::Enum)?;
        let name = self.expect_ident()?;
        let type_params = self.parse_type_params()?;
        self.expect(Token::LBrace)?;
        let mut variants = vec![];
        while !self.check(&Token::RBrace) && self.peek().is_some() {
            let vname = self.expect_ident()?;
            let fields = if self.match_token(&Token::LParen) {
                let mut fs = vec![];
                while !self.check(&Token::RParen) && self.peek().is_some() {
                    fs.push(self.parse_type()?);
                    if !self.match_token(&Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::RParen)?;
                fs
            } else {
                vec![]
            };
            variants.push(EnumVariant { name: vname, fields });
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        self.expect(Token::RBrace)?;
        Ok(Stmt::Enum {
            name,
            type_params,
            variants,
        })
    }

    fn parse_mod(&mut self) -> Result<Stmt> {
        self.expect(Token::Mod)?;
        let name = self.expect_ident()?;
        let items = self.parse_block()?;
        Ok(Stmt::Mod { name, items })
    }

    fn parse_impl(&mut self) -> Result<Stmt> {
        self.expect(Token::Impl)?;
        let target = self.expect_ident()?;
        let trait_name = if self.match_token(&Token::For) {
            Some(self.expect_ident()?)
        } else {
            None
        };
        self.expect(Token::LBrace)?;
        let mut methods = vec![];
        while !self.check(&Token::RBrace) && self.peek().is_some() {
            methods.push(self.parse_fn()?);
        }
        self.expect(Token::RBrace)?;
        Ok(Stmt::Impl {
            target,
            trait_name,
            methods,
        })
    }

    fn parse_trait(&mut self) -> Result<Stmt> {
        self.expect(Token::Trait)?;
        let name = self.expect_ident()?;
        self.expect(Token::LBrace)?;
        let mut methods = vec![];
        while !self.check(&Token::RBrace) && self.peek().is_some() {
            self.expect(Token::Fn)?;
            let mname = self.expect_ident()?;
            self.expect(Token::LParen)?;
            let params = self.parse_params()?;
            self.expect(Token::RParen)?;
            let return_type = if self.match_token(&Token::Arrow) {
                Some(self.parse_type()?)
            } else {
                None
            };
            self.semi()?;
            methods.push(TraitMethod {
                name: mname,
                params,
                return_type,
            });
        }
        self.expect(Token::RBrace)?;
        Ok(Stmt::Trait { name, methods })
    }

    fn parse_try(&mut self) -> Result<Stmt> {
        self.expect(Token::Try)?;
        let body = self.parse_block()?;
        let catch_name;
        let catch_body;
        if self.match_token(&Token::Catch) {
            catch_name = if let Some(Token::Ident(n)) = self.peek() {
                let n = n.clone();
                self.advance();
                Some(n)
            } else {
                None
            };
            catch_body = self.parse_block()?;
        } else {
            catch_name = None;
            catch_body = vec![];
        }
        Ok(Stmt::Try {
            body,
            catch_name,
            catch_body,
        })
    }

    fn parse_type_alias(&mut self) -> Result<Stmt> {
        self.expect(Token::Type)?;
        let name = self.expect_ident()?;
        self.expect(Token::Eq)?;
        let alias = self.parse_type()?;
        self.semi()?;
        Ok(Stmt::TypeAlias { name, alias })
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.peek() {
            Some(Token::Ident(n)) => {
                let n = n.clone();
                self.advance();
                Ok(n)
            }
            _ => Err(RakError::Parser("Expected identifier".to_string())),
        }
    }

    fn parse_type_params(&mut self) -> Result<Vec<String>> {
        let mut tps = vec![];
        if self.match_token(&Token::Lt) {
            while !self.check(&Token::Gt) && self.peek().is_some() {
                tps.push(self.expect_ident()?);
                if !self.match_token(&Token::Comma) {
                    break;
                }
            }
            self.expect(Token::Gt)?;
        }
        Ok(tps)
    }

    fn parse_params_with_generics(&mut self) -> Result<(Vec<Param>, Vec<String>)> {
        let type_params = if self.match_token(&Token::Lt) {
            let mut tps = vec![];
            while !self.check(&Token::Gt) && self.peek().is_some() {
                tps.push(self.expect_ident()?);
                if !self.match_token(&Token::Comma) {
                    break;
                }
            }
            self.expect(Token::Gt)?;
            tps
        } else {
            vec![]
        };
        self.expect(Token::LParen)?;
        let params = self.parse_params()?;
        self.expect(Token::RParen)?;
        Ok((params, type_params))
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>> {
        self.expect(Token::LBrace)?;
        self.parse_stmts_until_rbrace()
    }

    fn parse_stmts_until_rbrace(&mut self) -> Result<Vec<Stmt>> {
        let mut stmts = vec![];
        while !self.check(&Token::RBrace) && self.peek().is_some() {
            stmts.push(self.parse_stmt()?);
        }
        self.expect(Token::RBrace)?;
        Ok(stmts)
    }

    fn parse_options(&mut self) -> Result<Vec<(String, Expr)>> {
        let mut options = vec![];
        while !self.check(&Token::RBrace) && self.peek().is_some() {
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
        while !self.check(&Token::RParen) && !self.check(&Token::RBrace) && self.peek().is_some() {
            let name = self.expect_ident()?;
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
        let base = match self.peek() {
            Some(Token::Ident(name)) => {
                let name = name.clone();
                self.advance();
                match name.as_str() {
                    "hex8" => Type::Hex(8),
                    "hex16" => Type::Hex(16),
                    "hex32" => Type::Hex(32),
                    "hex64" => Type::Hex(64),
                    "int" => Type::Int,
                    "i8" => Type::I8,
                    "i16" => Type::I16,
                    "i32" => Type::I32,
                    "i64" => Type::I64,
                    "u8" => Type::U8,
                    "u16" => Type::U16,
                    "u32" => Type::U32,
                    "u64" => Type::U64,
                    "f32" => Type::F32,
                    "f64" => Type::F64,
                    "string" => Type::String,
                    "bytes" => Type::Bytes,
                    "bool" => Type::Bool,
                    "nil" => Type::Nil,
                    "Option" => {
                        if self.match_token(&Token::Lt) {
                            let inner = self.parse_type()?;
                            self.expect(Token::Gt)?;
                            Type::Option(Box::new(inner))
                        } else {
                            Type::Custom(name)
                        }
                    }
                    "Result" => {
                        if self.match_token(&Token::Lt) {
                            let ok = self.parse_type()?;
                            self.expect(Token::Comma)?;
                            let err = self.parse_type()?;
                            self.expect(Token::Gt)?;
                            Type::Result(Box::new(ok), Box::new(err))
                        } else {
                            Type::Custom(name)
                        }
                    }
                    _ => Type::Custom(name),
                }
            }
            _ => return Err(RakError::Parser("Expected type".to_string())),
        };
        if self.match_token(&Token::LBracket) {
            self.expect(Token::RBracket)?;
            return Ok(Type::Array(Box::new(base)));
        }
        Ok(base)
    }

    fn parse_match_expr(&mut self) -> Result<Expr> {
        self.expect(Token::Match)?;
        let value = self.parse_expr()?;
        self.expect(Token::LBrace)?;
        let mut arms = vec![];
        while !self.check(&Token::RBrace) && self.peek().is_some() {
            let pattern = self.parse_pattern()?;
            let guard = if self.match_token(&Token::If) {
                Some(self.parse_expr()?)
            } else {
                None
            };
            self.expect(Token::FatArrow)?;
            let body_expr = self.parse_expr()?;
            let body = vec![Stmt::Expr(Box::new(body_expr))];
            arms.push((pattern, guard, body));
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        self.expect(Token::RBrace)?;
        Ok(Expr::Match {
            value: Box::new(value),
            arms,
        })
    }

    fn parse_pattern(&mut self) -> Result<Pattern> {
        if self.match_token(&Token::Pipe) {
            let mut opts = vec![];
            loop {
                opts.push(self.parse_single_pattern()?);
                if !self.match_token(&Token::Pipe) {
                    break;
                }
            }
            return Ok(Pattern::Or(opts));
        }
        self.parse_single_pattern()
    }

    fn parse_single_pattern(&mut self) -> Result<Pattern> {
        match self.peek() {
            Some(Token::Ident(n)) => {
                let n = n.clone();
                self.advance();
                if n == "_" {
                    return Ok(Pattern::Wild);
                }
                if self.check(&Token::LParen) {
                    self.advance();
                    let mut inner = vec![];
                    while !self.check(&Token::RParen) && self.peek().is_some() {
                        inner.push(self.parse_pattern()?);
                        if !self.match_token(&Token::Comma) {
                            break;
                        }
                    }
                    self.expect(Token::RParen)?;
                    match n.as_str() {
                        "Some" => return Ok(Pattern::Some(Box::new(inner.into_iter().next().unwrap_or(Pattern::Wild)))),
                        "Ok" => return Ok(Pattern::Ok(Box::new(inner.into_iter().next().unwrap_or(Pattern::Wild)))),
                        "Err" => return Ok(Pattern::Err(Box::new(inner.into_iter().next().unwrap_or(Pattern::Wild)))),
                        _ => {}
                    }
                }
                if self.check(&Token::LBrace) {
                    self.advance();
                    let mut fields = vec![];
                    while !self.check(&Token::RBrace) && self.peek().is_some() {
                        let fname = self.expect_ident()?;
                        let fp = if self.match_token(&Token::Colon) {
                            self.parse_pattern()?
                        } else {
                            Pattern::Ident(fname.clone())
                        };
                        fields.push((fname, fp));
                        if !self.match_token(&Token::Comma) {
                            break;
                        }
                    }
                    self.expect(Token::RBrace)?;
                    return Ok(Pattern::Struct(n, fields));
                }
                match n.as_str() {
                    "None" => Ok(Pattern::None),
                    "Some" | "Ok" | "Err" => {
                        let inner = if self.match_token(&Token::LParen) {
                            let p = self.parse_pattern()?;
                            self.expect(Token::RParen)?;
                            Box::new(p)
                        } else {
                            Box::new(Pattern::Wild)
                        };
                        match n.as_str() {
                            "Some" => Ok(Pattern::Some(inner)),
                            "Ok" => Ok(Pattern::Ok(inner)),
                            "Err" => Ok(Pattern::Err(inner)),
                            _ => unreachable!(),
                        }
                    }
                    _ => Ok(Pattern::Ident(n)),
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
            Some(Token::Float(f)) => {
                let f = *f;
                self.advance();
                Ok(Pattern::Ident(format!("{}", f)))
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
            Some(Token::LParen) => {
                self.advance();
                let mut inner = vec![];
                while !self.check(&Token::RParen) && self.peek().is_some() {
                    inner.push(self.parse_pattern()?);
                    if !self.match_token(&Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::RParen)?;
                Ok(Pattern::Tuple(inner))
            }
            Some(Token::LBracket) => {
                self.advance();
                let mut inner = vec![];
                while !self.check(&Token::RBracket) && self.peek().is_some() {
                    inner.push(self.parse_pattern()?);
                    if !self.match_token(&Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::RBracket)?;
                Ok(Pattern::Array(inner))
            }
            _ => Err(RakError::Parser("Expected pattern".to_string())),
        }
    }

    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> Result<Expr> {
        let mut expr = self.parse_or()?;
        if self.match_token(&Token::Eq) {
            let value = self.parse_expr()?;
            if let Expr::Ident(name) = expr {
                expr = Expr::Assign(name, Box::new(value));
            } else {
                return Err(RakError::Parser("Invalid assignment target".to_string()));
            }
        } else if self.match_token(&Token::PlusEq) {
            let value = self.parse_expr()?;
            if let Expr::Ident(name) = expr {
                expr = Expr::CompoundAssign(CompoundOp::Add, name, Box::new(value));
            }
        } else if self.match_token(&Token::MinusEq) {
            let value = self.parse_expr()?;
            if let Expr::Ident(name) = expr {
                expr = Expr::CompoundAssign(CompoundOp::Sub, name, Box::new(value));
            }
        } else if self.match_token(&Token::StarEq) {
            let value = self.parse_expr()?;
            if let Expr::Ident(name) = expr {
                expr = Expr::CompoundAssign(CompoundOp::Mul, name, Box::new(value));
            }
        } else if self.match_token(&Token::SlashEq) {
            let value = self.parse_expr()?;
            if let Expr::Ident(name) = expr {
                expr = Expr::CompoundAssign(CompoundOp::Div, name, Box::new(value));
            }
        } else if self.match_token(&Token::PercentEq) {
            let value = self.parse_expr()?;
            if let Expr::Ident(name) = expr {
                expr = Expr::CompoundAssign(CompoundOp::Rem, name, Box::new(value));
            }
        }
        if self.match_token(&Token::Question) {
            expr = Expr::TryExpr(Box::new(expr));
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
        let mut left = self.parse_range()?;
        loop {
            if self.match_token(&Token::Lt) {
                let right = self.parse_range()?;
                left = Expr::Binary(BinOp::Lt, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::Gt) {
                let right = self.parse_range()?;
                left = Expr::Binary(BinOp::Gt, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::LtEq) {
                let right = self.parse_range()?;
                left = Expr::Binary(BinOp::LtEq, Box::new(left), Box::new(right));
            } else if self.match_token(&Token::GtEq) {
                let right = self.parse_range()?;
                left = Expr::Binary(BinOp::GtEq, Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_range(&mut self) -> Result<Expr> {
        let left = self.parse_bitwise()?;
        if self.match_token(&Token::DotDot) {
            if self.check(&Token::RBracket)
                || self.check(&Token::RParen)
                || self.check(&Token::Comma)
                || self.check(&Token::RBrace)
                || self.check(&Token::Semi)
                || self.peek().is_none()
            {
                return Ok(Expr::Range(Some(Box::new(left)), None));
            }
            let right = self.parse_bitwise()?;
            Ok(Expr::Range(Some(Box::new(left)), Some(Box::new(right))))
        } else {
            Ok(left)
        }
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
        } else if self.match_token(&Token::Await) {
            let expr = self.parse_unary()?;
            Ok(Expr::Await(Box::new(expr)))
        } else if self.match_token(&Token::Spawn) {
            let expr = self.parse_unary()?;
            Ok(Expr::Spawn(Box::new(expr)))
        } else if self.match_token(&Token::Raise) {
            let expr = self.parse_unary()?;
            Ok(Expr::Raise(Box::new(expr)))
        } else {
            self.parse_postfix()
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr> {
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
                if matches!(self.peek(), Some(Token::Int(_))) {
                    if let Some(Token::Int(i)) = self.peek() {
                        let i = *i;
                        self.advance();
                        expr = Expr::Index(Box::new(expr), Box::new(Expr::Int(i)));
                        continue;
                    }
                }
                let field = self.expect_ident()?;
                expr = Expr::FieldAccess(Box::new(expr), field);
            } else if self.match_token(&Token::ColonColon) {
                let seg = self.expect_ident()?;
                let mut path = match expr {
                    Expr::Path(p) => p,
                    Expr::Ident(n) => vec![n],
                    other => return Err(RakError::Parser(format!("Invalid path base: {:?}", other))),
                };
                path.push(seg);
                expr = Expr::Path(path);
            } else if self.match_token(&Token::LBracket) {
                let idx = self.parse_expr()?;
                self.expect(Token::RBracket)?;
                expr = Expr::Index(Box::new(expr), Box::new(idx));
            } else if self.match_token(&Token::Question) {
                expr = Expr::TryExpr(Box::new(expr));
            } else if self.match_token(&Token::As) {
                let t = self.parse_type()?;
                expr = Expr::As(Box::new(expr), t);
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_args(&mut self) -> Result<Vec<Expr>> {
        let mut args = vec![];
        while !self.check(&Token::RParen) && self.peek().is_some() {
            args.push(self.parse_expr()?);
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.peek().cloned() {
            Some(Token::Hex(h)) => {
                self.advance();
                Ok(Expr::Hex(h))
            }
            Some(Token::Int(i)) => {
                self.advance();
                Ok(Expr::Int(i))
            }
            Some(Token::Float(f)) => {
                self.advance();
                Ok(Expr::Float(f))
            }
            Some(Token::Float32(f)) => {
                self.advance();
                Ok(Expr::Float32(f))
            }
            Some(Token::TypedInt(d)) => {
                let d = d.clone();
                self.advance();
                Ok(Expr::TypedInt(d.value, map_int_kind(d.kind)))
            }
            Some(Token::String(s)) => {
                self.advance();
                Ok(Expr::String(s))
            }
            Some(Token::Interp(template)) => {
                self.advance();
                let parts = parse_interp_parts(&template)?;
                Ok(Expr::Interp {
                    template: template.replace("{{", "{").replace("}}", "}"),
                    parts,
                })
            }
            Some(Token::Bytes(b)) => {
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
            Some(Token::If) => self.parse_if_expr(),
            Some(Token::Match) => self.parse_match_expr(),
            Some(Token::Fn) => self.parse_lambda(),
            Some(Token::LParen) => {
                self.advance();
                if self.check(&Token::RParen) {
                    self.advance();
                    return Ok(Expr::Tuple(vec![]));
                }
                let first = self.parse_expr()?;
                if self.match_token(&Token::Comma) {
                    let mut elems = vec![first];
                    while !self.check(&Token::RParen) && self.peek().is_some() {
                        elems.push(self.parse_expr()?);
                        if !self.match_token(&Token::Comma) {
                            break;
                        }
                    }
                    self.expect(Token::RParen)?;
                    Ok(Expr::Tuple(elems))
                } else {
                    self.expect(Token::RParen)?;
                    Ok(first)
                }
            }
            Some(Token::LBracket) => {
                self.advance();
                let mut elements = vec![];
                while !self.check(&Token::RBracket) && self.peek().is_some() {
                    elements.push(self.parse_expr()?);
                    if !self.match_token(&Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::RBracket)?;
                Ok(Expr::Array(elements))
            }
            Some(Token::LBrace) => self.parse_map_or_block(),
            Some(Token::Ident(name)) => {
                self.advance();
                if self.check(&Token::ColonColon) {
                    let mut path = vec![name];
                    while self.match_token(&Token::ColonColon) {
                        path.push(self.expect_ident()?);
                    }
                    return Ok(Expr::Path(path));
                }
                if self.check(&Token::LBrace) && self.looks_like_struct_lit() {
                    self.advance();
                    let mut fields = vec![];
                    while !self.check(&Token::RBrace) && self.peek().is_some() {
                        let fname = self.expect_ident()?;
                        self.expect(Token::Colon)?;
                        let fval = self.parse_expr()?;
                        fields.push((fname, fval));
                        if !self.match_token(&Token::Comma) {
                            break;
                        }
                    }
                    self.expect(Token::RBrace)?;
                    return Ok(Expr::StructLit { name, fields });
                }
                Ok(Expr::Ident(name))
            }
            Some(tok) => Err(RakError::Parser(format!(
                "Unexpected token in expression: {:?}",
                tok
            ))),
            None => Err(RakError::Parser("Unexpected end of input".to_string())),
        }
    }

    fn looks_like_struct_lit(&self) -> bool {
        if self.check(&Token::LBrace) {
            if self.peek_n(1).map(|t| matches!(t, Token::RBrace)).unwrap_or(false) {
                return true;
            }
            matches!(self.peek_n(1), Some(Token::Ident(_)))
                && matches!(self.peek_n(2), Some(Token::Colon))
        } else {
            false
        }
    }

    fn parse_map_or_block(&mut self) -> Result<Expr> {
        self.advance();
        if self.check(&Token::RBrace) {
            self.advance();
            return Ok(Expr::Map(vec![]));
        }
        if matches!(self.peek(), Some(Token::Ident(_)))
            && matches!(self.peek_n(1), Some(Token::Colon))
        {
            let mut pairs = vec![];
            while !self.check(&Token::RBrace) && self.peek().is_some() {
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
                pairs.push((key, value));
                if !self.match_token(&Token::Comma) {
                    break;
                }
            }
            self.expect(Token::RBrace)?;
            return Ok(Expr::Map(pairs));
        }
        let mut stmts = vec![];
        while !self.check(&Token::RBrace) && self.peek().is_some() {
            stmts.push(self.parse_stmt()?);
        }
        self.expect(Token::RBrace)?;
        if stmts.len() == 1 {
            if let Stmt::Expr(e) = &stmts[0] {
                return Ok((**e).clone());
            }
        }
        Ok(Expr::Block(stmts))
    }

    fn parse_lambda(&mut self) -> Result<Expr> {
        self.expect(Token::Fn)?;
        let is_async = self.match_token(&Token::Async);
        let (params, _) = self.parse_params_with_generics()?;
        let return_type = if self.match_token(&Token::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let _ = return_type;
        if self.check(&Token::LBrace) {
            let body = self.parse_block()?;
            Ok(Expr::Function {
                params,
                return_type: None,
                body,
                captures: vec![],
                is_async,
            })
        } else {
            let body_expr = self.parse_expr()?;
            Ok(Expr::Lambda {
                params,
                body: Box::new(body_expr),
                captures: vec![],
            })
        }
    }
}

fn parse_interp_parts(template: &str) -> Result<Vec<Expr>> {
    let mut parts = vec![];
    let mut chars = template.chars().peekable();
    let mut buf = String::new();
    while let Some(c) = chars.next() {
        if c == '{' {
            if chars.peek() == Some(&'{') {
                chars.next();
                buf.push('{');
                continue;
            }
            let mut depth = 1;
            let mut expr_src = String::new();
            while let Some(c2) = chars.next() {
                if c2 == '{' {
                    depth += 1;
                    expr_src.push(c2);
                } else if c2 == '}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    expr_src.push(c2);
                } else {
                    expr_src.push(c2);
                }
            }
            if !expr_src.is_empty() {
                parts.push(parse_expr_str(&expr_src)?);
            }
        } else if c == '}' {
            if chars.peek() == Some(&'}') {
                chars.next();
                buf.push('}');
                continue;
            }
            buf.push(c);
        } else {
            buf.push(c);
        }
    }
    let _ = buf;
    Ok(parts)
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

    #[test]
    fn test_parse_float() {
        let source = "let x = 3.14;";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens).unwrap();
        match &module.items[0] {
            Stmt::Let { value, .. } => assert!(matches!(**value, Expr::Float(_))),
            _ => panic!(),
        }
    }

    #[test]
    fn test_parse_tuple() {
        let source = "let p = (1, 2, 3);";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens).unwrap();
        match &module.items[0] {
            Stmt::Let { value, .. } => assert!(matches!(&**value, Expr::Tuple(t) if t.len() == 3)),
            _ => panic!(),
        }
    }

    #[test]
    fn test_parse_interp() {
        let source = "let x = f\"hello {name}!\";";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens).unwrap();
        match &module.items[0] {
            Stmt::Let { value, .. } => assert!(matches!(**value, Expr::Interp { .. })),
            _ => panic!(),
        }
    }

    #[test]
    fn test_parse_if_expr() {
        let source = "let x = if true { 1 } else { 2 };";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens).unwrap();
        match &module.items[0] {
            Stmt::Let { value, .. } => assert!(matches!(**value, Expr::If { .. })),
            _ => panic!(),
        }
    }

    #[test]
    fn test_parse_struct_lit() {
        let source = "let p = Point { x: 1, y: 2 };";
        let tokens = tokenize(source).unwrap();
        let module = parse(&tokens).unwrap();
        match &module.items[0] {
            Stmt::Let { value, .. } => assert!(matches!(**value, Expr::StructLit { .. })),
            _ => panic!(),
        }
    }
}

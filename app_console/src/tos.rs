use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

macro_rules! print {
    (target: $target:expr, $($arg:tt)+) => {};
    ($($arg:tt)*) => {};
}
macro_rules! println {
    (target: $target:expr, $($arg:tt)+) => {};
    ($($arg:tt)*) => {};
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Plus,
    Minus,
}

pub type NI = usize;

#[derive(Debug, Clone, PartialEq)]
pub enum N {
    FuncCall {
        func: NI,
        args: Vec<NI>,
    },
    Block(Vec<NI>),
    Set {
        name: String,
        val: NI,
    },
    FuncDef {
        args_name: Vec<String>,
        scope: Vec<NI>,
    },
    Get {
        name: String,
    },
    Unit,
    Num(f64),
    Binary {
        op: Op,
        l: NI,
        r: NI,
    },
}

pub struct Ctx {
    pub ast_vec: Vec<N>,
    pub variables: BTreeMap<String, N>,
    pub path: String,
}

impl Ctx {
    pub fn get_n(&self, idx: usize) -> N {
        return self.ast_vec[idx].clone();
    }
}

pub fn eval<'a>(ni: &NI, ctx: &'a mut Box<Ctx>) -> N {
    let n = ctx.get_n(*ni);

    let res = match n {
        N::Block(arr) => {
            ctx.path.push('_');
            let mut res = N::Unit;
            for a in arr.iter() {
                res = eval(a, ctx);
            }
            ctx.path.pop();
            res
        }
        N::Set { name, val } => {
            let val = eval(&val, ctx);
            let key = format!("{}{}", ctx.path, name);
            ctx.variables.insert(key, val);
            N::Unit
        }
        N::Get { name } => {
            //
            let mut res = None;
            let mut poped = vec![];
            loop {
                let key = format!("{}{}", ctx.path, name);
                res = ctx.variables.get(&key);
                match res {
                    Some(r) => {
                        for i in 0..poped.len() {
                            ctx.path.push(poped[poped.len() - i - 1]);
                        }
                        break r.clone();
                    }
                    None => {
                        let pop = ctx.path.pop();
                        if pop.is_none() {
                            panic!("NO")
                        }
                        poped.push(pop.unwrap())
                    }
                }
            }
        }
        N::FuncCall { func, args } => {
            //
            match eval(&func, ctx) {
                N::FuncDef { args_name, scope } => {
                    let mut path_after = ctx.path.clone();
                    path_after.push('_');

                    for (i, arg) in args.iter().enumerate() {
                        let val = eval(arg, ctx);
                        println!("fun call arg{}: {:?}", i, val);
                        let key = format!("{}{}", path_after, args_name[i]);
                        ctx.variables.insert(key, val);
                    }
                    ctx.path.push('_');
                    let mut res = N::Unit;
                    for a in scope.iter() {
                        res = eval(a, ctx);
                    }
                    ctx.path.pop();
                    res
                }
                _ => N::Unit,
            }
        }

        N::Binary { op, l, r } => {
            let lt = eval(&l, ctx);
            let rt = eval(&r, ctx);
            match (op, &lt, &rt) {
                (Op::Plus, N::Num(li), N::Num(ri)) => N::Num(li + ri),
                (Op::Minus, N::Num(li), N::Num(ri)) => N::Num(li - ri),
                _ => {
                    println!("ERROR: bin {:?} {:?}", lt, rt);
                    N::Unit
                }
            }
        }
        e => e.clone(),
    };

    res
}

pub fn print_ast(ni: &usize, ast: &Vec<N>, pad: String, inline: bool) {
    let n = &ast[*ni];

    match n {
        N::Block(arr) => {
            if !inline {
                print!("{}", pad);
            }
            println!("{{");

            for a in arr {
                print_ast(a, ast, format!("  {}", pad), false);
                println!(" ");
            }
            println!("{}}}", pad);
        }
        N::Binary { op, l, r } => {
            if !inline {
                print!("{}", pad);
            }
            print!("(");
            print_ast(l, ast, "".to_owned(), true);
            print!(" {:?} ", op);
            print_ast(r, ast, "".to_owned(), true);
            print!(")");
        }
        N::Get { name } => {
            print!("{}", name);
        }
        N::Num(num) => {
            print!("{}", num);
        }
        N::FuncDef { args_name, scope } => {
            if !inline {
                print!("{}", pad);
            }

            print!("({}) =>", args_name.join(","));
            for a in scope {
                print_ast(a, ast, format!("  {}", pad), true);
                println!("");
            }
        }

        N::FuncCall { func, args } => {
            if !inline {
                print!("{}", pad);
            }
            print!("func#{}(", func);
            for arg in args.iter() {
                print_ast(arg, ast, pad.clone(), true);
                print!(",");
            }
            println!(")");
        }
        N::Set { name, val } => {
            print!("{}let {} =", pad, name);
            print_ast(val, ast, pad, true);
        }
        e => {
            println!("{}{:?}", pad, e);
        }
    }
}

pub fn next_token(i: &mut usize, code: &[char]) -> Token {
    let skip_whitespace = |i: &mut usize| {
        while code[*i] == ' ' || code[*i] == '\n' {
            *i = *i + 1;
        }
    };
    let skip_comma = |i: &mut usize| {
        if code[*i] == ',' {
            *i = *i + 1;
        }
    };

    let parse_number = |i: &mut usize| {
        let backup_i = *i;
        let mut id = "".to_owned();
        while code[*i].is_ascii_digit() || code[*i] == '.' {
            id = format!("{}{}", id, code[*i]);
            *i += 1;
        }
        if id.len() > 0 {
            if let Ok(j) = id.parse::<f64>() {
                Some(j)
            } else {
                *i = backup_i;
                None
            }
        } else {
            *i = backup_i;
            None
        }
    };

    let parse_ident = |i: &mut usize| {
        let mut id = "".to_owned();
        while code[*i].is_alphanumeric() || code[*i] == '_' {
            id = format!("{}{}", id, code[*i]);
            *i += 1;
        }
        if id.len() > 0 {
            Some(id)
        } else {
            None
        }
    };
    loop {
        if *i >= code.len() {
            break Token::Err("i>code".to_owned());
        }

        skip_whitespace(i);

        let c = code[*i];

        {
            if c == '{' {
                *i += 1;
                break Token::BlockStart;
            }
            if let '}' = c {
                *i += 1;
                break Token::BlockEnd;
            }
            if c == 'l'
                && *i + 3 < code.len()
                && code[*i + 1] == 'e'
                && code[*i + 2] == 't'
                && code[*i + 3] == ' '
            {
                *i += 4;
                skip_whitespace(i);
                let id = match parse_ident(i) {
                    Some(id) => id,
                    None => break Token::Err("no id after let # ".to_owned()),
                };
                skip_whitespace(i);

                if code[*i] != '=' {
                    break Token::Err("no equal after let 'id' # ".to_owned());
                }
                *i += 1;

                break Token::Let(id);
            }

            if c == '(' {
                let i_backup = *i;
                *i += 1;

                let mut idents = vec![];
                loop {
                    skip_whitespace(i);
                    match parse_ident(i) {
                        Some(id) => idents.push(id),
                        None => break,
                    };
                    skip_comma(i);
                }

                skip_whitespace(i);

                if code[*i] != ')' {
                    break Token::Err("no end parenthesis after args".to_owned());
                }
                *i += 1;
                skip_whitespace(i);

                if code[*i] != '=' || code[*i + 1] != '>' {
                    break Token::Err("no => after args".to_owned());
                }
                *i += 2;

                break Token::FuncDefStart { args: idents };
            }

            if let Some(num) = parse_number(i) {
                break Token::N(N::Num(num));
            }

            if let Some(id) = parse_ident(i) {
                skip_whitespace(i);
                if code[*i] == '(' {
                    *i += 1;
                    break Token::FuncCallStart(id);
                }

                break Token::N(N::Get { name: id });
            }
            if c == ',' {
                *i += 1;
                break Token::Comma;
            }
            if c == ')' {
                *i += 1;
                break Token::ParEnd;
            }
            if c == '+' {
                *i += 1;
                break Token::Bin(Op::Plus);
            }
            if c == '-' {
                *i += 1;
                break Token::Bin(Op::Minus);
            }
        }

        *i += 1;
    }
}

pub fn insert_in_p(ast_vec: &mut Vec<N>, parent: NI, idx: NI) {
    match &mut ast_vec[parent] {
        N::Block(v) => {
            //
            v.push(idx);
        }
        N::FuncDef { args_name, scope } => {
            scope.push(idx);
        }
        N::FuncCall { func, args } => {
            args.push(idx);
        }
        _ => {}
    }
}

fn pa(i: usize) -> String {
    return format!("{:width$}", "", width = i * 3);
}

pub fn parse_expr(ast_vec: &mut Vec<N>, i: &mut usize, code: &[char], pad: usize) -> Option<NI> {
    if *i >= code.len() {
        return None;
    }
    print!("{}", pa(pad));
    println!("parse expr {:?}", &code[*i..(*i + 5).min(code.len() - 1)]);
    let term = parse_term(ast_vec, i, code, pad + 1);

    if term.is_none() {
        return None;
    }
    let term = term.unwrap();

    let mut j = *i;
    let token = next_token(&mut j, code);
    if let Token::Bin(op) = token {
        print!("{}", pa(pad));
        println!("got bin");

        *i = j;

        let term_right = parse_expr(ast_vec, i, code, pad + 1).expect("No right");

        let n = N::Binary {
            op: op,
            l: term,
            r: term_right,
        };
        let block_ni = ast_vec.len();
        ast_vec.push(n);
        return Some(block_ni);
    }

    Some(term)
}
pub fn parse_term(ast_vec: &mut Vec<N>, i: &mut usize, code: &[char], pad: usize) -> Option<NI> {
    if *i >= code.len() {
        return None;
    }
    print!("{}", pa(pad));
    println!("parse_term {:?}", &code[*i..(*i + 5).min(code.len() - 1)]);
    let token = next_token(i, code);

    if token == Token::BlockStart {
        print!("{}", pa(pad));
        println!("Block");
        let block_ni = ast_vec.len();
        ast_vec.push(N::Block(vec![]));

        loop {
            let mut j = *i;
            let e = parse_expr(ast_vec, &mut j, code, pad + 1);
            match e {
                Some(expr) => {
                    *i = j;
                    insert_in_p(ast_vec, block_ni, expr)
                }
                None => {
                    break;
                }
            }
        }
        let token = next_token(i, code);
        if token == Token::BlockEnd {
            return Some(block_ni);
        } else {
            panic!("No block end")
        }
    }

    if let Token::FuncDefStart { args } = token {
        print!("{}", pa(pad));
        println!("FuncDefStart");

        let scope = parse_expr(ast_vec, i, code, pad + 1).expect("No func body");

        let n = N::FuncDef {
            args_name: args,
            scope: vec![scope],
        };

        let block_ni: usize = ast_vec.len();
        ast_vec.push(n);

        print!("{}", pa(pad));
        println!("FuncDefStart END");
        return Some(block_ni);
    }

    if let Token::Let(name) = token {
        print!("{}", pa(pad));
        println!("Let");
        let val = parse_expr(ast_vec, i, code, pad + 1).expect("No expr after let");
        let n = N::Set { name, val };
        let set_expr_ni: usize = ast_vec.len();
        ast_vec.push(n);
        return Some(set_expr_ni);
    }

    if let Token::N(N::Num(num)) = token {
        print!("{}", pa(pad));
        println!("Num");
        let n = N::Num(num);
        let expr_ni: usize = ast_vec.len();
        ast_vec.push(n);
        return Some(expr_ni);
    }

    if let Token::N(N::Get { name }) = token {
        print!("{}", pa(pad));
        println!("Get");
        let n = N::Get { name };
        let expr_ni: usize = ast_vec.len();
        ast_vec.push(n);
        return Some(expr_ni);
    }

    if let Token::FuncCallStart(name) = token {
        print!("{}", pa(pad));
        println!("FuncCallStart");

        let get_ni = {
            let get = N::Get { name };
            let expr_ni: usize = ast_vec.len();
            ast_vec.push(get);
            expr_ni
        };

        let n = N::FuncCall {
            func: get_ni,
            args: vec![],
        };
        let expr_ni: usize = ast_vec.len();
        ast_vec.push(n);
        loop {
            let mut j = *i;
            let e = parse_expr(ast_vec, &mut j, code, pad + 1);
            match e {
                Some(expr) => {
                    *i = j;
                    insert_in_p(ast_vec, expr_ni, expr);
                    let mut k = *i;
                    let token = next_token(&mut k, code);
                    if token == Token::Comma {
                        *i = k
                    }
                }
                None => {
                    break;
                }
            }
        }
        let token = next_token(i, code);
        if token == Token::ParEnd {
            return Some(expr_ni);
        } else {
            panic!("No block end")
        }
    }

    println!("UNMATCHED {:?}", token);

    None
}
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    BlockStart,
    BlockEnd,
    Comma,
    ParEnd,
    Bin(Op),
    N(N),
    Let(String),
    Err(String),
    FuncCallStart(String),
    FuncDefStart { args: Vec<String> },
}

//! Random grammatical Loom code generator for `loom_fuzz`.

use crate::prng::Rng;

/// Generates a random grammatical Loom source file string.
#[must_use]
pub fn generate(rng: &mut Rng) -> String {
    let mut generator = Generator { depth: 0 };
    generator.gen_file(rng)
}

struct Generator {
    depth: usize,
}

impl Generator {
    fn gen_file(&mut self, rng: &mut Rng) -> String {
        let num_items = rng.gen_range(1, 4);
        let mut items = Vec::new();
        for _ in 0..num_items {
            items.push(self.gen_item(rng));
        }
        items.join("\n\n")
    }

    fn gen_item(&mut self, rng: &mut Rng) -> String {
        if self.depth > 5 {
            return "const X: Int = 1".to_string();
        }
        self.depth += 1;
        let res = match rng.gen_range(0, 5) {
            0 => {
                let name = Self::gen_ident(rng);
                let body = self.gen_stmt(rng);
                format!("def {name}(x: Int) -> Int\n  {body}\nend")
            }
            1 => {
                let name = Self::gen_type_ident(rng);
                format!("struct {name}\n  x: Int\n  y: String\nend")
            }
            2 => {
                let name = Self::gen_type_ident(rng);
                format!("enum {name}\n  A\n  B(Int)\nend")
            }
            3 => {
                let name = Self::gen_ident(rng);
                let body = self.gen_stmt(rng);
                format!("module {name}\n  def foo()\n    {body}\n  end\nend")
            }
            _ => {
                let name = Self::gen_ident(rng);
                let val = self.gen_expr(rng);
                format!("const {name}: Int = {val}")
            }
        };
        self.depth -= 1;
        res
    }

    fn gen_stmt(&mut self, rng: &mut Rng) -> String {
        if self.depth > 5 {
            return "var x = 1".to_string();
        }
        self.depth += 1;
        let res = match rng.gen_range(0, 5) {
            0 => {
                let name = Self::gen_ident(rng);
                let val = self.gen_expr(rng);
                format!("var {name} = {val}")
            }
            1 => {
                let cond = self.gen_expr(rng);
                let body1 = self.gen_stmt(rng);
                let body2 = self.gen_stmt(rng);
                format!("if {cond} then\n    {body1}\n  else\n    {body2}\n  end")
            }
            2 => {
                let cond = self.gen_expr(rng);
                let body = self.gen_stmt(rng);
                format!("while {cond} do\n    {body}\n  end")
            }
            3 => {
                let val = self.gen_expr(rng);
                format!("return {val}")
            }
            _ => self.gen_expr(rng),
        };
        self.depth -= 1;
        res
    }

    fn gen_expr(&mut self, rng: &mut Rng) -> String {
        if self.depth > 5 {
            return rng.gen_range(0, 100).to_string();
        }
        self.depth += 1;
        let res = match rng.gen_range(0, 7) {
            0 => rng.gen_range(0, 100).to_string(),
            1 => Self::gen_ident(rng),
            2 => format!("\"{}\"", Self::gen_ident(rng)),
            3 => {
                let left = self.gen_expr(rng);
                let right = self.gen_expr(rng);
                let op = *rng.choose(&["+", "-", "*", "/", "==", "!=", "and", "or", ".."]);
                format!("({left} {op} {right})")
            }
            4 => {
                let name = Self::gen_ident(rng);
                let arg = self.gen_expr(rng);
                format!("{name}({arg})")
            }
            5 => {
                let e1 = self.gen_expr(rng);
                let e2 = self.gen_expr(rng);
                format!("[{e1}, {e2}]")
            }
            _ => {
                let param = Self::gen_ident(rng);
                let body = self.gen_expr(rng);
                format!("{{ |{param}| {body} }}")
            }
        };
        self.depth -= 1;
        res
    }

    fn gen_ident(rng: &mut Rng) -> String {
        let names = ["x", "y", "z", "a", "b", "foo", "bar", "val", "item"];
        (*rng.choose(&names)).to_string()
    }

    fn gen_type_ident(rng: &mut Rng) -> String {
        let names = ["MyType", "Point", "Color", "Data", "Node", "Option"];
        (*rng.choose(&names)).to_string()
    }
}
